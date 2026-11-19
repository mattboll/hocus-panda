#!/usr/bin/env python3
"""Makes the Hocuspanda camera's face and hand models (docs/design/camera.md,
lot 4) from MediaPipe's published ones.

    scripts/camera/landmarks-to-onnx.py face_landmarker.task \\
        hand_landmarker.task crates/hocus-camera/models/

The two files are MediaPipe's face landmarker (with blendshapes) and hand
landmarker, Apache-2.0 (crates/hocus-camera/models/LICENSE-MEDIAPIPE), as
published:

    https://storage.googleapis.com/mediapipe-models/face_landmarker/face_landmarker/float16/1/face_landmarker.task
    SHA-256 64184e229b263107bc2b804c6625db1341ff2bb731874b0bcc2fe6544e0bc9ff
    https://storage.googleapis.com/mediapipe-models/hand_landmarker/hand_landmarker/float16/1/hand_landmarker.task
    SHA-256 fbc2a30080c3c557093b5ddfc334698132eb341044ccee322ccf8bcf3607cde1

Each is a zip of TensorFlow Lite models; tract, which runs them, reads
ONNX. So each model's graph is written again, operator by operator, as
ONNX (opset 13), its weights the same numbers: stored as 16-bit floats as
MediaPipe stores them, each cast to 32 bits once as tract loads it.
Nothing is trained or changed. A picture's input is planar,
`1 × 3 × H × W`, red, green, blue from 0 to 1; the outputs have the
TensorFlow Lite model's shapes. Written:

    face_detector.onnx            the face's box (BlazeFace, 128 × 128)
    face_landmarks.onnx           its 478 points (256 × 256)
    face_blendshapes.onnx         its 52 expressions, from 146 of the points
    hand_detector.onnx            the palm's box (192 × 192)
    hand_landmarks.onnx           its 21 points (224 × 224)

Needs the `tflite`, `onnx` and `numpy` Python packages (pip, Apache-2.0
and BSD).
"""

import hashlib
import io
import os
import sys
import zipfile

import numpy as np
import onnx
import tflite
from onnx import TensorProto, helper, numpy_helper

TASKS = {
    "face_landmarker.task": (
        "64184e229b263107bc2b804c6625db1341ff2bb731874b0bcc2fe6544e0bc9ff",
        {
            "face_detector.tflite": "face_detector.onnx",
            "face_landmarks_detector.tflite": "face_landmarks.onnx",
            "face_blendshapes.tflite": "face_blendshapes.onnx",
        },
    ),
    "hand_landmarker.task": (
        "fbc2a30080c3c557093b5ddfc334698132eb341044ccee322ccf8bcf3607cde1",
        {
            "hand_detector.tflite": "hand_detector.onnx",
            "hand_landmarks_detector.tflite": "hand_landmarks.onnx",
        },
    ),
}

OPS = {v: k for k, v in tflite.BuiltinOperator.__dict__.items() if not k.startswith("_")}
NHWC_TO_NCHW = [0, 3, 1, 2]
NCHW_TO_NHWC = [0, 2, 3, 1]


def options(op, kind):
    o = op.BuiltinOptions()
    opts = kind()
    if o is None:
        return None
    opts.Init(o.Bytes, o.Pos)
    return opts


class Converter:
    """One TensorFlow Lite graph written as ONNX. A four-dimensional
    activation is kept channels first (NCHW) where the operators that read
    it want that, and turned back (NHWC) where the graph's own layout
    matters (a reshape, a concatenation, a mean): each tensor's layout is
    known, and a transpose is added where it changes."""

    def __init__(self, raw, label):
        self.model = tflite.Model.GetRootAsModel(raw, 0)
        self.graph = self.model.Subgraphs(0)
        self.label = label
        self.nodes = []
        self.inits = []
        self.consts = {}
        # Tensor index: "nchw" or "plain" (the model's own layout).
        self.layout = {}
        self.names = {}
        self.count = 0

    # --- tensors --------------------------------------------------------

    def tensor(self, i):
        return self.graph.Tensors(i)

    def shape(self, i):
        shape = self.tensor(i).ShapeAsNumpy()
        # A scalar's shape reads as 0.
        return [] if isinstance(shape, int) else [int(v) for v in shape]

    def fresh(self, base):
        self.count += 1
        return f"{base}__{self.count}"

    def data(self, i):
        """A constant tensor's numbers, or None for an activation."""
        if i in self.consts:
            return self.consts[i]
        t = self.tensor(i)
        buf = self.model.Buffers(t.Buffer()).DataAsNumpy()
        if isinstance(buf, int) or buf.size == 0:
            return None
        kind = {
            tflite.TensorType.FLOAT32: np.float32,
            tflite.TensorType.FLOAT16: np.float16,
            tflite.TensorType.INT32: np.int32,
            tflite.TensorType.INT64: np.int64,
        }[t.Type()]
        return np.frombuffer(buf.tobytes(), dtype=kind).reshape(self.shape(i))

    def weight(self, array, base):
        """A float constant: stored as 16 bits, cast to 32 as it loads."""
        label = self.fresh(base)
        self.inits.append(
            numpy_helper.from_array(np.ascontiguousarray(array.astype(np.float16)), label + "_h")
        )
        self.nodes.append(helper.make_node("Cast", [label + "_h"], [label], to=TensorProto.FLOAT))
        return label

    def ints(self, values, base):
        label = self.fresh(base)
        self.inits.append(numpy_helper.from_array(np.array(values, dtype=np.int64), label))
        return label

    def name(self, i, want=None):
        """Tensor `i`'s ONNX name, in layout `want` ("nchw" or "plain";
        either when None)."""
        const = self.data(i)
        if const is not None:
            key = ("const", i, want)
            if key not in self.names:
                self.names[key] = self.constant(const, i, want)
            return self.names[key]
        have = self.layout[i]
        base = self.names[i]
        if want is None or want == have or len(self.shape(i)) != 4:
            return base
        key = (i, want)
        if key not in self.names:
            label = self.fresh(base + "_" + want)
            perm = NHWC_TO_NCHW if want == "nchw" else NCHW_TO_NHWC
            self.nodes.append(helper.make_node("Transpose", [base], [label], perm=perm))
            self.names[key] = label
        return self.names[key]

    def constant(self, const, i, want):
        """A constant as an initializer: a float one in layout `want` (a
        channels-first activation's operand is turned as it is)."""
        if const.dtype in (np.float16, np.float32):
            if want == "nchw":
                c = const.astype(np.float32)
                c = c.reshape([1] * (4 - c.ndim) + list(c.shape)).transpose(NHWC_TO_NCHW)
                return self.weight(c, f"c{i}")
            return self.weight(const, f"c{i}")
        return self.ints(const.astype(np.int64), f"c{i}")

    def out(self, i, layout):
        label = self.tensor(i).Name().decode() or f"t{i}"
        self.names[i] = label
        self.layout[i] = layout if len(self.shape(i)) == 4 else "plain"
        return label

    def act(self, target, fused):
        """The fused activation after an operator: the operator writes to
        the name returned, the activation to `target`."""
        if fused == tflite.ActivationFunctionType.NONE:
            return target
        pre = self.fresh(target + "_pre")
        if fused == tflite.ActivationFunctionType.RELU:
            self.nodes.append(helper.make_node("Relu", [pre], [target]))
        elif fused == tflite.ActivationFunctionType.RELU6:
            self.nodes.append(
                helper.make_node(
                    "Clip", [pre, self.weight(np.float32(0), "zero"), self.weight(np.float32(6), "six")],
                    [target],
                )
            )
        else:
            sys.exit(f"{self.label}: activation {fused} not written")
        return pre

    @staticmethod
    def same_pads(size, k, s, d=1):
        k = (k - 1) * d + 1
        out = -(-size // s)
        total = max((out - 1) * s + k - size, 0)
        return total // 2, total - total // 2

    # --- operators ------------------------------------------------------

    def convert(self):
        g = self.graph
        for j in range(g.InputsLength()):
            i = g.Inputs(j)
            self.out(i, "nchw")
        for n in range(g.OperatorsLength()):
            op = g.Operators(n)
            code = self.model.OperatorCodes(op.OpcodeIndex())
            kind = OPS.get(max(code.BuiltinCode(), code.DeprecatedBuiltinCode()))
            ins = [op.Inputs(j) for j in range(op.InputsLength())]
            outs = [op.Outputs(j) for j in range(op.OutputsLength())]
            getattr(self, "op_" + kind.lower(), self.unknown(kind))(op, ins, outs)
        inputs, outputs = [], []
        for j in range(g.InputsLength()):
            i = g.Inputs(j)
            s = self.shape(i)
            if len(s) == 4:
                s = [s[k] for k in NHWC_TO_NCHW]
            inputs.append(helper.make_tensor_value_info(self.names[i], TensorProto.FLOAT, s))
        for j in range(g.OutputsLength()):
            i = g.Outputs(j)
            label = self.tensor(i).Name().decode()
            have = self.name(i, "plain")
            final = f"output_{j}"
            self.nodes.append(helper.make_node("Identity", [have], [final]))
            outputs.append(helper.make_tensor_value_info(final, TensorProto.FLOAT, self.shape(i)))
            del label
        model = helper.make_model(
            helper.make_graph(self.sorted(), self.label, inputs, outputs, self.inits),
            opset_imports=[helper.make_opsetid("", 13)],
            producer_name="hocuspanda scripts/camera/landmarks-to-onnx.py",
        )
        model.doc_string = (
            f"MediaPipe's {self.label}, Apache-2.0, written as ONNX by "
            "scripts/camera/landmarks-to-onnx.py"
        )
        onnx.checker.check_model(model)
        return model

    def sorted(self):
        """The nodes in an order where each comes after what it reads (a
        fused activation is written before its operator)."""
        known = {i.name for i in self.inits}
        known.update(self.names[self.graph.Inputs(j)] for j in range(self.graph.InputsLength()))
        known.add("")
        left, done = list(self.nodes), []
        while left:
            ready = [n for n in left if all(i in known for i in n.input)]
            if not ready:
                sys.exit(f"{self.label}: a node reads what nothing writes")
            for n in ready:
                done.append(n)
                known.update(n.output)
            taken = {id(n) for n in ready}
            left = [n for n in left if id(n) not in taken]
        return done

    def unknown(self, kind):
        def fail(*_):
            sys.exit(f"{self.label}: {kind} not written")

        return fail

    def op_dequantize(self, op, ins, outs):
        self.consts[outs[0]] = self.data(ins[0]).astype(np.float32)

    def conv(self, op, ins, outs, depthwise):
        opts = options(op, tflite.DepthwiseConv2DOptions if depthwise else tflite.Conv2DOptions)
        weights = self.data(ins[1]).astype(np.float32)
        bias = self.data(ins[2]) if len(ins) > 2 and ins[2] >= 0 else None
        h, w = self.shape(ins[0])[1:3]
        if depthwise:
            kernel = weights.transpose(3, 0, 1, 2)  # 1, H, W, C to C, 1, H, W
            group = kernel.shape[0]
            if opts.DepthMultiplier() not in (0, 1):
                sys.exit(f"{self.label}: a depth multiplier")
        else:
            kernel = weights.transpose(0, 3, 1, 2)  # O, H, W, I to O, I, H, W
            group = 1
        kh, kw = kernel.shape[2:]
        sh, sw = opts.StrideH(), opts.StrideW()
        dh, dw = opts.DilationHFactor() or 1, opts.DilationWFactor() or 1
        if opts.Padding() == tflite.Padding.SAME:
            top, bottom = self.same_pads(h, kh, sh, dh)
            left, right = self.same_pads(w, kw, sw, dw)
        else:
            top = bottom = left = right = 0
        target = self.out(outs[0], "nchw")
        pre = self.act(target, opts.FusedActivationFunction())
        args = [self.name(ins[0], "nchw"), self.weight(kernel, target + "_w")]
        if bias is not None:
            args.append(self.weight(bias.astype(np.float32), target + "_b"))
        self.nodes.append(
            helper.make_node(
                "Conv",
                args,
                [pre],
                kernel_shape=[kh, kw],
                strides=[sh, sw],
                dilations=[dh, dw],
                pads=[top, left, bottom, right],
                group=group,
            )
        )

    def op_conv_2d(self, op, ins, outs):
        self.conv(op, ins, outs, False)

    def op_depthwise_conv_2d(self, op, ins, outs):
        self.conv(op, ins, outs, True)

    def op_max_pool_2d(self, op, ins, outs):
        opts = options(op, tflite.Pool2DOptions)
        h, w = self.shape(ins[0])[1:3]
        kh, kw = opts.FilterHeight(), opts.FilterWidth()
        sh, sw = opts.StrideH(), opts.StrideW()
        if opts.Padding() == tflite.Padding.SAME:
            top, bottom = self.same_pads(h, kh, sh)
            left, right = self.same_pads(w, kw, sw)
        else:
            top = bottom = left = right = 0
        target = self.out(outs[0], "nchw")
        pre = self.act(target, opts.FusedActivationFunction())
        self.nodes.append(
            helper.make_node(
                "MaxPool",
                [self.name(ins[0], "nchw")],
                [pre],
                kernel_shape=[kh, kw],
                strides=[sh, sw],
                pads=[top, left, bottom, right],
            )
        )

    def op_pad(self, op, ins, outs):
        pads = self.data(ins[1]).astype(np.int64)  # rank × (before, after)
        if len(self.shape(ins[0])) == 4:
            pads = pads[NHWC_TO_NCHW]
            source = self.name(ins[0], "nchw")
            layout = "nchw"
        else:
            source = self.name(ins[0], "plain")
            layout = "plain"
        flat = list(pads[:, 0]) + list(pads[:, 1])
        target = self.out(outs[0], layout)
        self.nodes.append(
            helper.make_node("Pad", [source, self.ints(flat, target + "_pads")], [target])
        )

    def op_prelu(self, op, ins, outs):
        alpha = self.data(ins[1]).astype(np.float32)
        if len(self.shape(ins[0])) == 4:
            c = alpha.reshape(-1)
            if c.size == 1:
                slope = c.reshape(1)
            else:
                slope = alpha.reshape([1] * (4 - alpha.ndim) + list(alpha.shape)).transpose(
                    NHWC_TO_NCHW
                )[0]
            source, layout = self.name(ins[0], "nchw"), "nchw"
        else:
            slope, source, layout = alpha, self.name(ins[0], "plain"), "plain"
        target = self.out(outs[0], layout)
        # As max(x, 0) + slope · min(x, 0): tract runs ONNX's PRelu as a
        # comparison, a choice and a product apart, some five times
        # slower than these.
        positive, negative, scaled = (self.fresh(target + k) for k in ("_pos", "_neg", "_scaled"))
        zero = self.weight(np.zeros(1, np.float32), target + "_zero")

        def scale_by(x, slope, out):
            # Each channel times its slope: as a 1 × 1 convolution of a
            # channel each, which tract runs much faster than a product
            # broadcast along the channels.
            if layout == "nchw" and slope.size > 1:
                kernel = slope.reshape(-1, 1, 1, 1)
                return helper.make_node(
                    "Conv",
                    [x, self.weight(kernel, target + "_slope")],
                    [out],
                    kernel_shape=[1, 1],
                    group=kernel.shape[0],
                )
            return helper.make_node("Mul", [x, self.weight(slope, target + "_slope")], [out])

        self.nodes += [
            helper.make_node("Relu", [source], [positive]),
            helper.make_node("Min", [source, zero], [negative]),
            scale_by(negative, slope, scaled),
            helper.make_node("Add", [positive, scaled], [target]),
        ]

    def unary(self, onnx_op, ins, outs):
        i = ins[0]
        layout = self.layout.get(i, "plain")
        target = self.out(outs[0], layout)
        self.nodes.append(helper.make_node(onnx_op, [self.name(i, layout)], [target]))

    def op_relu(self, op, ins, outs):
        self.unary("Relu", ins, outs)

    def op_logistic(self, op, ins, outs):
        self.unary("Sigmoid", ins, outs)

    def op_neg(self, op, ins, outs):
        self.unary("Neg", ins, outs)

    def op_sqrt(self, op, ins, outs):
        self.unary("Sqrt", ins, outs)

    def op_rsqrt(self, op, ins, outs):
        i = ins[0]
        layout = self.layout.get(i, "plain")
        target = self.out(outs[0], layout)
        root = self.fresh(target + "_sqrt")
        self.nodes.append(helper.make_node("Sqrt", [self.name(i, layout)], [root]))
        self.nodes.append(helper.make_node("Reciprocal", [root], [target]))

    def binary_inputs(self, ins):
        """Both inputs in one layout: channels first if an activation of
        four dimensions is, constants turned to match."""
        acts = [i for i in ins if self.data(i) is None]
        nchw = any(self.layout.get(i) == "nchw" and len(self.shape(i)) == 4 for i in acts)
        layout = "nchw" if nchw else "plain"
        names = []
        for i in ins:
            if self.data(i) is None:
                if layout == "nchw" and len(self.shape(i)) != 4:
                    sys.exit(f"{self.label}: a broadcast across layouts")
                names.append(self.name(i, layout))
            else:
                names.append(self.name(i, layout if layout == "nchw" else None))
        return names, layout

    def binary(self, onnx_op, op, ins, outs, opts_kind=None):
        names, layout = self.binary_inputs(ins)
        target = self.out(outs[0], layout)
        fused = 0
        if opts_kind is not None:
            opts = options(op, opts_kind)
            if opts is not None:
                fused = opts.FusedActivationFunction()
        pre = self.act(target, fused)
        self.nodes.append(helper.make_node(onnx_op, names, [pre]))

    def op_add(self, op, ins, outs):
        self.binary("Add", op, ins, outs, tflite.AddOptions)

    def op_mul(self, op, ins, outs):
        self.binary("Mul", op, ins, outs, tflite.MulOptions)

    def op_sub(self, op, ins, outs):
        self.binary("Sub", op, ins, outs, tflite.SubOptions)

    def op_div(self, op, ins, outs):
        self.binary("Div", op, ins, outs, tflite.DivOptions)

    def op_squared_difference(self, op, ins, outs):
        names, layout = self.binary_inputs(ins)
        target = self.out(outs[0], layout)
        diff = self.fresh(target + "_diff")
        self.nodes.append(helper.make_node("Sub", names, [diff]))
        self.nodes.append(helper.make_node("Mul", [diff, diff], [target]))

    def op_resize_bilinear(self, op, ins, outs):
        opts = options(op, tflite.ResizeBilinearOptions)
        if opts.AlignCorners():
            mode = "align_corners"
        elif opts.HalfPixelCenters():
            mode = "half_pixel"
        else:
            mode = "asymmetric"
        oh, ow = (int(v) for v in self.data(ins[1]))
        target = self.out(outs[0], "nchw")
        sizes = self.ints([1, self.shape(ins[0])[3], oh, ow], target + "_sizes")
        self.nodes.append(
            helper.make_node(
                "Resize",
                [self.name(ins[0], "nchw"), "", "", sizes],
                [target],
                mode="linear",
                coordinate_transformation_mode=mode,
            )
        )

    def op_reshape(self, op, ins, outs):
        target = self.out(outs[0], "plain")
        shape = self.ints(self.shape(outs[0]), target + "_shape")
        self.nodes.append(helper.make_node("Reshape", [self.name(ins[0], "plain"), shape], [target]))

    def op_transpose(self, op, ins, outs):
        perm = [int(v) for v in self.data(ins[1])]
        target = self.out(outs[0], "plain")
        self.nodes.append(
            helper.make_node("Transpose", [self.name(ins[0], "plain")], [target], perm=perm)
        )

    def op_concatenation(self, op, ins, outs):
        opts = options(op, tflite.ConcatenationOptions)
        axis = opts.Axis()
        rank = len(self.shape(outs[0]))
        if axis < 0:
            axis += rank
        all_nchw = rank == 4 and all(
            self.data(i) is None and self.layout.get(i) == "nchw" for i in ins
        )
        layout = "nchw" if all_nchw else "plain"
        if all_nchw:
            axis = NHWC_TO_NCHW.index(axis)
        target = self.out(outs[0], layout)
        pre = self.act(target, opts.FusedActivationFunction())
        self.nodes.append(
            helper.make_node("Concat", [self.name(i, layout) for i in ins], [pre], axis=axis)
        )

    def reduce(self, onnx_op, op, ins, outs):
        opts = options(op, tflite.ReducerOptions)
        keep = 1 if opts is not None and opts.KeepDims() else 0
        axes = [int(v) for v in np.atleast_1d(self.data(ins[1]))]
        target = self.out(outs[0], "plain")
        self.nodes.append(
            helper.make_node(
                onnx_op, [self.name(ins[0], "plain")], [target], axes=axes, keepdims=keep
            )
        )

    def op_mean(self, op, ins, outs):
        self.reduce("ReduceMean", op, ins, outs)

    def op_sum(self, op, ins, outs):
        # ReduceSum takes its axes as an input from opset 13.
        opts = options(op, tflite.ReducerOptions)
        keep = 1 if opts is not None and opts.KeepDims() else 0
        axes = [int(v) for v in np.atleast_1d(self.data(ins[1]))]
        target = self.out(outs[0], "plain")
        self.nodes.append(
            helper.make_node(
                "ReduceSum",
                [self.name(ins[0], "plain"), self.ints(axes, target + "_axes")],
                [target],
                keepdims=keep,
            )
        )

    def op_strided_slice(self, op, ins, outs):
        opts = options(op, tflite.StridedSliceOptions)
        if opts.EllipsisMask() or opts.NewAxisMask():
            sys.exit(f"{self.label}: a strided slice with an ellipsis or new axes")
        shape = self.shape(ins[0])
        begin = [int(v) for v in self.data(ins[1])]
        end = [int(v) for v in self.data(ins[2])]
        steps = [int(v) for v in self.data(ins[3])]
        big = 2**31 - 1
        for k in range(len(begin)):
            if opts.BeginMask() & (1 << k):
                begin[k] = 0 if steps[k] > 0 else big
            if opts.EndMask() & (1 << k):
                end[k] = big if steps[k] > 0 else -big
            if opts.ShrinkAxisMask() & (1 << k):
                end[k] = begin[k] + 1 if begin[k] != -1 else shape[k]
        target = self.out(outs[0], "plain")
        sliced = self.fresh(target + "_slice") if opts.ShrinkAxisMask() else target
        axes = list(range(len(begin)))
        self.nodes.append(
            helper.make_node(
                "Slice",
                [
                    self.name(ins[0], "plain"),
                    self.ints(begin, target + "_b"),
                    self.ints(end, target + "_e"),
                    self.ints(axes, target + "_a"),
                    self.ints(steps, target + "_s"),
                ],
                [sliced],
            )
        )
        if opts.ShrinkAxisMask():
            squeeze = [k for k in axes if opts.ShrinkAxisMask() & (1 << k)]
            self.nodes.append(
                helper.make_node("Squeeze", [sliced, self.ints(squeeze, target + "_sq")], [target])
            )

    def op_fully_connected(self, op, ins, outs):
        opts = options(op, tflite.FullyConnectedOptions)
        weights = self.data(ins[1]).astype(np.float32)  # O, I
        bias = self.data(ins[2]) if len(ins) > 2 and ins[2] >= 0 else None
        out_shape = self.shape(outs[0])
        target = self.out(outs[0], "plain")
        flat = self.fresh(target + "_flat")
        self.nodes.append(
            helper.make_node(
                "Reshape",
                [self.name(ins[0], "plain"), self.ints([-1, weights.shape[1]], flat + "_s")],
                [flat],
            )
        )
        pre = self.act(target, opts.FusedActivationFunction())
        gemm = self.fresh(target + "_gemm") if len(out_shape) != 2 else pre
        args = [flat, self.weight(weights, target + "_w")]
        if bias is not None:
            args.append(self.weight(bias.astype(np.float32), target + "_b"))
        self.nodes.append(helper.make_node("Gemm", args, [gemm], transB=1))
        if gemm != pre:
            self.nodes.append(
                helper.make_node("Reshape", [gemm, self.ints(out_shape, target + "_os")], [pre])
            )


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    folder = sys.argv[3]
    for path in sys.argv[1:3]:
        raw = open(path, "rb").read()
        base = os.path.basename(path)
        if base not in TASKS:
            sys.exit(f"{base}: not one of {', '.join(TASKS)}")
        sha, members = TASKS[base]
        if hashlib.sha256(raw).hexdigest() != sha:
            sys.exit(f"{base}: not the model expected (SHA-256)")
        with zipfile.ZipFile(io.BytesIO(raw)) as bundle:
            for member, written in members.items():
                label = written.removesuffix(".onnx")
                model = Converter(bundle.read(member), label).convert()
                with open(os.path.join(folder, written), "wb") as f:
                    f.write(model.SerializeToString())
                print(f"{written}: {len(model.graph.node)} nodes", file=sys.stderr)


if __name__ == "__main__":
    main()
