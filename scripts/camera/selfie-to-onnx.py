#!/usr/bin/env python3
"""Makes the Hocuspanda camera's person segmentation model
(docs/design/camera.md, lot 3) from MediaPipe's published one.

    scripts/camera/selfie-to-onnx.py MODEL.tflite \\
        > crates/hocus-camera/models/selfie_segmentation_landscape.onnx

MODEL.tflite is MediaPipe's selfie segmentation, landscape (144 × 256),
Apache-2.0 (crates/hocus-camera/models/LICENSE-MEDIAPIPE), as published:

    https://storage.googleapis.com/mediapipe-assets/selfie_segmentation_landscape.tflite
    SHA-256 a77d03f4659b9f6b6c1f5106947bf40e99d7655094b6527f214ea7d451106edd

tract, which runs it, reads ONNX; this model's TensorFlow Lite holds a
custom operator of MediaPipe's (`Convolution2DTransposeBias`), which no
TensorFlow Lite reader but MediaPipe's knows. So its graph is written
again, operator by operator, as ONNX (opset 13), its weights the same
numbers (stored as 16-bit floats, written as 32-bit): nothing is trained
or changed. The input is planar, `1 × 3 × 144 × 256`, red, green, blue
from 0 to 1; the output `1 × 1 × 144 × 256`, each pixel's probability of
being the person.

Needs the `tflite`, `onnx` and `numpy` Python packages (pip, Apache-2.0
and BSD).
"""

import hashlib
import struct
import sys

import numpy as np
import onnx
import tflite
from onnx import TensorProto, helper, numpy_helper

SHA256 = "a77d03f4659b9f6b6c1f5106947bf40e99d7655094b6527f214ea7d451106edd"
OPS = {v: k for k, v in tflite.BuiltinOperator.__dict__.items() if not k.startswith("_")}


def main():
    raw = open(sys.argv[1], "rb").read()
    if hashlib.sha256(raw).hexdigest() != SHA256:
        sys.exit("not the model expected (SHA-256)")
    model = tflite.Model.GetRootAsModel(raw, 0)
    graph = model.Subgraphs(0)

    def tensor(i):
        return graph.Tensors(i)

    def name(i):
        return tensor(i).Name().decode()

    def shape(i):
        return [int(v) for v in tensor(i).ShapeAsNumpy()]

    def data(i):
        """A constant tensor's numbers, as 32-bit floats or 32-bit ints."""
        t = tensor(i)
        buf = model.Buffers(t.Buffer()).DataAsNumpy()
        kind = {
            tflite.TensorType.FLOAT32: np.float32,
            tflite.TensorType.FLOAT16: np.float16,
            tflite.TensorType.INT32: np.int32,
        }[t.Type()]
        return np.frombuffer(buf.tobytes(), dtype=kind).reshape(shape(i))

    nodes, inits = [], []
    # A float16 constant's DEQUANTIZE: its output is the constant itself.
    consts = {}

    def const(i):
        if i in consts:
            return consts[i]
        return data(i)

    def init(label, array):
        inits.append(numpy_helper.from_array(np.ascontiguousarray(array), label))
        return label

    def nchw(i):
        """A tensor's name in the graph (activations are NCHW here)."""
        return name(i)

    def same_pads(size, k, s):
        out = -(-size // s)
        total = max((out - 1) * s + k - size, 0)
        return total // 2, total - total // 2

    def activation(out, fused):
        if fused == 0:
            return out
        if fused == 1:
            nodes.append(helper.make_node("Relu", [out + "_pre"], [out]))
            return out + "_pre"
        sys.exit(f"activation {fused} not written")

    for n in range(graph.OperatorsLength()):
        op = graph.Operators(n)
        code = model.OperatorCodes(op.OpcodeIndex())
        kind = OPS.get(max(code.BuiltinCode(), code.DeprecatedBuiltinCode()))
        ins = [op.Inputs(j) for j in range(op.InputsLength())]
        outs = [op.Outputs(j) for j in range(op.OutputsLength())]
        out = nchw(outs[0])
        if kind == "DEQUANTIZE":
            consts[outs[0]] = data(ins[0]).astype(np.float32)
        elif kind in ("CONV_2D", "DEPTHWISE_CONV_2D"):
            opts = (tflite.Conv2DOptions if kind == "CONV_2D" else tflite.DepthwiseConv2DOptions)()
            o = op.BuiltinOptions()
            opts.Init(o.Bytes, o.Pos)
            weights = const(ins[1])
            bias = const(ins[2])
            h, w = shape(ins[0])[1:3]
            if kind == "CONV_2D":
                # O, H, W, I to O, I, H, W.
                kernel = weights.transpose(0, 3, 1, 2)
                group = 1
            else:
                # 1, H, W, C to C, 1, H, W.
                kernel = weights.transpose(3, 0, 1, 2)
                group = kernel.shape[0]
            kh, kw = kernel.shape[2:]
            sh, sw = opts.StrideH(), opts.StrideW()
            if opts.Padding() == tflite.Padding.SAME:
                top, bottom = same_pads(h, kh, sh)
                left, right = same_pads(w, kw, sw)
            else:
                top = bottom = left = right = 0
            target = activation(out, opts.FusedActivationFunction())
            nodes.append(
                helper.make_node(
                    "Conv",
                    [nchw(ins[0]), init(out + "_w", kernel), init(out + "_b", bias)],
                    [target],
                    kernel_shape=[kh, kw],
                    strides=[sh, sw],
                    pads=[top, left, bottom, right],
                    group=group,
                )
            )
        elif kind == "CUSTOM" and code.CustomCode().decode() == "Convolution2DTransposeBias":
            # Its options: TfLiteTransposeConvParams (padding, stride w, h).
            padding, sw, sh = struct.unpack("<iii", op.CustomOptionsAsNumpy().tobytes()[:12])
            weights = const(ins[1])  # O, H, W, I
            bias = const(ins[2])
            kernel = weights.transpose(3, 0, 1, 2)  # I, O, H, W
            kh, kw = kernel.shape[2:]
            h, w = shape(ins[0])[1:3]
            oh, ow = shape(outs[0])[1:3]
            th = max(sh * (h - 1) + kh - oh, 0)
            tw = max(sw * (w - 1) + kw - ow, 0)
            if padding != 1:
                sys.exit("a transposed convolution not padded SAME")
            nodes.append(
                helper.make_node(
                    "ConvTranspose",
                    [nchw(ins[0]), init(out + "_w", kernel), init(out + "_b", bias)],
                    [out],
                    kernel_shape=[kh, kw],
                    strides=[sh, sw],
                    pads=[th // 2, tw // 2, th - th // 2, tw - tw // 2],
                )
            )
        elif kind == "RELU":
            nodes.append(helper.make_node("Relu", [nchw(ins[0])], [out]))
        elif kind == "HARD_SWISH":
            nodes.append(
                helper.make_node(
                    "HardSigmoid", [nchw(ins[0])], [out + "_gate"], alpha=1.0 / 6.0, beta=0.5
                )
            )
            nodes.append(helper.make_node("Mul", [nchw(ins[0]), out + "_gate"], [out]))
        elif kind == "LOGISTIC":
            nodes.append(helper.make_node("Sigmoid", [nchw(ins[0])], [out]))
        elif kind in ("ADD", "MUL"):
            o = op.BuiltinOptions()
            opts = (tflite.AddOptions if kind == "ADD" else tflite.MulOptions)()
            if o is not None:
                opts.Init(o.Bytes, o.Pos)
                fused = opts.FusedActivationFunction()
            else:
                fused = 0
            target = activation(out, fused)
            nodes.append(
                helper.make_node(kind.capitalize(), [nchw(ins[0]), nchw(ins[1])], [target])
            )
        elif kind == "MEAN":
            axes = list(data(ins[1]))
            if axes != [1, 2]:
                sys.exit(f"a mean over {axes}")
            nodes.append(helper.make_node("GlobalAveragePool", [nchw(ins[0])], [out]))
        elif kind == "RESIZE_BILINEAR":
            opts = tflite.ResizeBilinearOptions()
            o = op.BuiltinOptions()
            opts.Init(o.Bytes, o.Pos)
            if opts.AlignCorners():
                mode = "align_corners"
            elif opts.HalfPixelCenters():
                mode = "half_pixel"
            else:
                mode = "asymmetric"
            oh, ow = (int(v) for v in data(ins[1]))
            sizes = init(out + "_sizes", np.array([1, shape(ins[0])[3], oh, ow], dtype=np.int64))
            nodes.append(
                helper.make_node(
                    "Resize",
                    [nchw(ins[0]), "", "", sizes],
                    [out],
                    mode="linear",
                    coordinate_transformation_mode=mode,
                )
            )
        else:
            sys.exit(f"{kind} not written")

    graph_in = graph.Inputs(0)
    graph_out = graph.Outputs(0)
    _, h, w, c = shape(graph_in)
    onnx_model = helper.make_model(
        helper.make_graph(
            nodes,
            "selfie_segmentation_landscape",
            [helper.make_tensor_value_info(nchw(graph_in), TensorProto.FLOAT, [1, c, h, w])],
            [helper.make_tensor_value_info(nchw(graph_out), TensorProto.FLOAT, [1, 1, h, w])],
            inits,
        ),
        opset_imports=[helper.make_opsetid("", 13)],
        producer_name="hocuspanda scripts/camera/selfie-to-onnx.py",
    )
    onnx_model.doc_string = (
        "MediaPipe's selfie segmentation (landscape), Apache-2.0, "
        "written as ONNX by scripts/camera/selfie-to-onnx.py"
    )
    onnx.checker.check_model(onnx_model)
    sys.stdout.buffer.write(onnx_model.SerializeToString())


if __name__ == "__main__":
    main()
