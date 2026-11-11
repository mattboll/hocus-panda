//! A share's frame read back on the GPU without the loop waiting or
//! copying: the loop asks for the frame's pixels into a pixel buffer of
//! its own (`glReadPixels` into a PBO, then an EGL fence and `glFlush`)
//! and goes on; a thread of its own, with a GL context shared with the
//! renderer's (made current there only, for good), waits for the fence
//! with a timeout, maps the buffer (Mesa may copy there: off the loop),
//! lends the mapped memory to the streams' thread ([`super::lease`]),
//! and unmaps it once given back, or after a timeout. A buffer whose
//! fence never signals is never mapped nor read into again. Each share has
//! three such buffers at most: with none free, the frame is owed until one
//! is.
//!
//! The loop only reads the share's texture through its own context, the
//! one that drew it (GL orders the two); the thread only touches the pixel
//! buffer, once the fence says the GPU wrote it.

use std::{
    collections::HashMap,
    ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

use smithay::backend::{
    egl::{EGLContext, fence::EGLFence},
    renderer::gles::{GlesRenderer, GlesTexture, ffi},
};

use super::sink::FrameTo;

/// Buffers a share reads into at most: one drawn, one being copied, one
/// spare.
const MOST: usize = 3;
/// How long the thread waits for the GPU, then for the streams' thread.
const WAIT: Duration = Duration::from_secs(1);

/// A pixel buffer of a share's: its GL name, its bytes, and whether it is
/// free (the thread unmapped it).
#[derive(Debug)]
struct Pbo {
    name: ffi::types::GLuint,
    len: usize,
    free: Arc<AtomicBool>,
}

/// What the thread is handed: a buffer read into, its fence, and where
/// the frame goes.
struct Job {
    id: u64,
    pbo: ffi::types::GLuint,
    fence: EGLFence,
    len: usize,
    size: (u32, u32),
    free: Arc<AtomicBool>,
    to: FrameTo,
}

/// The loop's side: each share's buffers, and the thread.
#[derive(Debug, Default)]
pub struct Reader {
    pbos: HashMap<u64, Vec<Pbo>>,
    thread: Option<mpsc::Sender<Job>>,
    /// The thread could not start (no shared context): none is tried
    /// again.
    failed: bool,
}

impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Job").field("id", &self.id).finish()
    }
}

/// What came of asking for a frame.
#[derive(Debug, PartialEq, Eq)]
pub enum Asked {
    /// The thread has it.
    Sent,
    /// No buffer free: owed until one is.
    Busy,
}

impl Reader {
    /// Share `id`'s frame, drawn into `texture` (`size` pixels), read back
    /// and handed to the thread for `to`.
    pub fn read(
        &mut self,
        renderer: &mut GlesRenderer,
        id: u64,
        texture: &GlesTexture,
        size: (u32, u32),
        to: &FrameTo,
    ) -> anyhow::Result<Asked> {
        let thread = self.thread(renderer)?;
        let len = size.0 as usize * size.1 as usize * 4;
        let pbos = self.pbos.entry(id).or_default();
        // Buffers of another size go once free.
        let mut stale = Vec::new();
        pbos.retain(|p| {
            let keep = p.len == len || !p.free.load(Ordering::Acquire);
            if !keep {
                stale.push(p.name);
            }
            keep
        });
        let at = pbos
            .iter()
            .position(|p| p.len == len && p.free.load(Ordering::Acquire));
        let make = at.is_none() && pbos.len() < MOST;
        if at.is_none() && !make {
            delete(renderer, &stale);
            return Ok(Asked::Busy);
        }
        let tex = texture.tex_id();
        let (w, h) = (size.0 as i32, size.1 as i32);
        let made = renderer.with_context(|gl| {
            // SAFETY: the renderer's context is current (`with_context`);
            // the names are made here or by this context's share group,
            // the texture is the renderer's own, and every binding is put
            // back to none before returning, as the renderer expects none.
            #[allow(unsafe_code)]
            unsafe {
                for name in &stale {
                    gl.DeleteBuffers(1, name);
                }
                let mut pbo = 0;
                if make {
                    gl.GenBuffers(1, &mut pbo);
                    gl.BindBuffer(ffi::PIXEL_PACK_BUFFER, pbo);
                    gl.BufferData(
                        ffi::PIXEL_PACK_BUFFER,
                        len as isize,
                        ptr::null(),
                        ffi::STREAM_READ,
                    );
                    gl.BindBuffer(ffi::PIXEL_PACK_BUFFER, 0);
                }
                let mut fbo = 0;
                gl.GetError();
                gl.GenFramebuffers(1, &mut fbo);
                gl.BindFramebuffer(ffi::FRAMEBUFFER, fbo);
                gl.FramebufferTexture2D(
                    ffi::FRAMEBUFFER,
                    ffi::COLOR_ATTACHMENT0,
                    ffi::TEXTURE_2D,
                    tex,
                    0,
                );
                (pbo, fbo)
            }
        })?;
        let (new, fbo) = made;
        let at = at.unwrap_or_else(|| {
            pbos.push(Pbo {
                name: new,
                len,
                free: Arc::new(AtomicBool::new(true)),
            });
            pbos.len() - 1
        });
        let pbo = pbos
            .get(at)
            .ok_or_else(|| anyhow::anyhow!("no buffer to read into"))?;
        let name = pbo.name;
        let free = pbo.free.clone();
        let read = renderer.with_context(|gl| {
            // SAFETY: as above; the pixel buffer holds `len` bytes, what
            // `w × h` pixels of 4 bytes read into it take.
            #[allow(unsafe_code)]
            unsafe {
                gl.BindBuffer(ffi::PIXEL_PACK_BUFFER, name);
                gl.ReadBuffer(ffi::COLOR_ATTACHMENT0);
                gl.ReadPixels(
                    0,
                    0,
                    w,
                    h,
                    ffi::BGRA_EXT,
                    ffi::UNSIGNED_BYTE,
                    ptr::null_mut(),
                );
                gl.ReadBuffer(ffi::NONE);
                gl.BindBuffer(ffi::PIXEL_PACK_BUFFER, 0);
                gl.BindFramebuffer(ffi::FRAMEBUFFER, 0);
                gl.DeleteFramebuffers(1, &fbo);
                gl.GetError()
            }
        })?;
        let error = read;
        anyhow::ensure!(
            error == ffi::NO_ERROR,
            "the frame not read back: GL error {error:#x}"
        );
        // The fence after the read, in the renderer's context (current
        // since `with_context`), flushed so that the GPU sees both.
        let fence = EGLFence::create(renderer.egl_context().display())?;
        renderer.with_context(|gl| {
            // SAFETY: the renderer's context is current.
            #[allow(unsafe_code)]
            unsafe {
                gl.Flush();
            }
        })?;
        free.store(false, Ordering::Release);
        let job = Job {
            id,
            pbo: name,
            fence,
            len,
            size,
            free: free.clone(),
            to: to.clone(),
        };
        if thread.send(job).is_err() {
            // Its buffer is left as it is: the GPU may still write it.
            self.thread = None;
            self.failed = true;
            anyhow::bail!("the read-back's thread is gone");
        }
        Ok(Asked::Sent)
    }

    /// The buffers of the shares that are gone go, once free.
    pub fn forget(&mut self, renderer: &mut GlesRenderer, keep: impl Fn(u64) -> bool) {
        let mut names = Vec::new();
        for (id, pbos) in &mut self.pbos {
            if keep(*id) {
                continue;
            }
            pbos.retain(|p| {
                let free = p.free.load(Ordering::Acquire);
                if free {
                    names.push(p.name);
                }
                !free
            });
        }
        self.pbos.retain(|_, pbos| !pbos.is_empty());
        delete(renderer, &names);
    }

    /// The thread, started the first time with a context shared with the
    /// renderer's.
    fn thread(&mut self, renderer: &mut GlesRenderer) -> anyhow::Result<mpsc::Sender<Job>> {
        if let Some(thread) = &self.thread {
            return Ok(thread.clone());
        }
        anyhow::ensure!(!self.failed, "no read-back thread");
        let shared =
            EGLContext::new_shared(renderer.egl_context().display(), renderer.egl_context());
        let context = match shared {
            Ok(context) => context,
            Err(err) => {
                self.failed = true;
                anyhow::bail!("no context shared with the renderer's: {err}");
            }
        };
        let (sender, jobs) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("screen cast read-back".to_owned())
            .spawn(move || {
                read_back(&context, &jobs);
                // The thread ends as the backend goes, its renderer and
                // its EGL display with it: destroying the context here,
                // while the display is terminated on the loop's thread,
                // corrupted Mesa's heap (llvmpipe, 5.10's tests). The
                // display's end frees it.
                std::mem::forget(context);
            })?;
        self.thread = Some(sender.clone());
        Ok(sender)
    }
}

fn delete(renderer: &mut GlesRenderer, names: &[ffi::types::GLuint]) {
    if names.is_empty() {
        return;
    }
    let deleted = renderer.with_context(|gl| {
        // SAFETY: the renderer's context is current; the names are pixel
        // buffers of its share group that nothing maps any more.
        #[allow(unsafe_code)]
        unsafe {
            for name in names {
                gl.DeleteBuffers(1, name);
            }
        }
    });
    if let Err(err) = deleted {
        tracing::warn!(%err, "a share's pixel buffers not deleted");
    }
}

/// The thread: each frame waited for, mapped, lent, then unmapped.
fn read_back(context: &EGLContext, jobs: &mpsc::Receiver<Job>) {
    // SAFETY: the context is this thread's alone, made current here once
    // for good (surfaceless, as Mesa offers).
    #[allow(unsafe_code)]
    if let Err(err) = unsafe { context.make_current() } {
        tracing::warn!(%err, "the read-back's context cannot be made current");
        return;
    }
    let gl = ffi::Gles2::load_with(|symbol| {
        // SAFETY: EGL's loader, with the context current.
        #[allow(unsafe_code)]
        unsafe {
            smithay::backend::egl::get_proc_address(symbol)
        }
    });
    while let Ok(job) = jobs.recv() {
        // The GPU wrote the buffer, or it is never mapped.
        match job.fence.client_wait(Some(WAIT), false) {
            Ok(true) => {}
            signaled => {
                tracing::warn!(
                    ?signaled,
                    id = job.id,
                    "a frame's read-back never ended: dropped"
                );
                job.to.returned(job.id);
                continue;
            }
        }
        let mapped = {
            // SAFETY: the buffer comes from the share group this context is
            // in, `len` bytes, the GPU done with it (the fence); the loop
            // does not read into it again before it is free.
            #[allow(unsafe_code)]
            unsafe {
                gl.BindBuffer(ffi::PIXEL_PACK_BUFFER, job.pbo);
                let at = gl.MapBufferRange(
                    ffi::PIXEL_PACK_BUFFER,
                    0,
                    job.len as isize,
                    ffi::MAP_READ_BIT,
                );
                gl.BindBuffer(ffi::PIXEL_PACK_BUFFER, 0);
                at
            }
        };
        if !mapped.is_null() {
            let pixels = {
                // SAFETY: the buffer is mapped for reading, `len` bytes,
                // until it is unmapped below, after the lease is back or
                // taken back.
                #[allow(unsafe_code)]
                unsafe {
                    std::slice::from_raw_parts(mapped as *const u8, job.len)
                }
            };
            let lease = job.to.lend(pixels, true);
            job.to
                .frame(job.id, lease.clone(), job.size, job.size.0 * 4);
            if !lease.wait_back(WAIT) {
                lease.revoke();
            }
            unmap(&gl, job.pbo);
        }
        job.free.store(true, Ordering::Release);
        job.to.returned(job.id);
    }
}

fn unmap(gl: &ffi::Gles2, pbo: ffi::types::GLuint) {
    // SAFETY: the buffer was mapped by this context, and nothing reads
    // its memory any more (the lease is back or taken back).
    #[allow(unsafe_code)]
    unsafe {
        gl.BindBuffer(ffi::PIXEL_PACK_BUFFER, pbo);
        gl.UnmapBuffer(ffi::PIXEL_PACK_BUFFER);
        gl.BindBuffer(ffi::PIXEL_PACK_BUFFER, 0);
    }
}
