//! The password checked through PAM (7.1, docs/design/lock.md), on the
//! lock's thread, never on the loop: `pam_start` on the service, then
//! `pam_authenticate`, `pam_acct_mgmt` and `pam_setcred(PAM_REFRESH_CRED)`.
//! The conversation answers the password prompt with the password given,
//! once, and takes the modules' messages without showing them.
//!
//! libpam is opened at run time (`libpam.so.0`): the compositor builds
//! without PAM's headers, and a system without PAM keeps the session
//! locked, saying why in the log. The FFI is here alone, each block with
//! what makes it sound.
#![allow(unsafe_code)]

use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    ptr,
};

use zeroize::Zeroize;

use super::auth::Outcome;

const PAM_SUCCESS: c_int = 0;
const PAM_BUF_ERR: c_int = 5;
const PAM_PERM_DENIED: c_int = 6;
const PAM_AUTH_ERR: c_int = 7;
const PAM_MAXTRIES: c_int = 11;
const PAM_CONV_ERR: c_int = 19;
const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_PROMPT_ECHO_ON: c_int = 2;
const PAM_ERROR_MSG: c_int = 3;
const PAM_TEXT_INFO: c_int = 4;
const PAM_REFRESH_CRED: c_int = 0x0010;

#[repr(C)]
struct Message {
    style: c_int,
    text: *const c_char,
}

#[repr(C)]
struct Response {
    resp: *mut c_char,
    retcode: c_int,
}

type Converse = unsafe extern "C" fn(
    count: c_int,
    messages: *mut *const Message,
    responses: *mut *mut Response,
    data: *mut c_void,
) -> c_int;

#[repr(C)]
struct Conversation {
    converse: Converse,
    data: *mut c_void,
}

type Start = unsafe extern "C" fn(
    service: *const c_char,
    user: *const c_char,
    conversation: *const Conversation,
    handle: *mut *mut c_void,
) -> c_int;
type Step = unsafe extern "C" fn(handle: *mut c_void, flags: c_int) -> c_int;
type End = unsafe extern "C" fn(handle: *mut c_void, status: c_int) -> c_int;

/// What the conversation knows: the password to give, and whether PAM
/// asked for it.
struct Talk {
    password: CString,
    asked: bool,
}

impl Drop for Talk {
    fn drop(&mut self) {
        // The password's bytes, zeroed as they go.
        let mut bytes = std::mem::take(&mut self.password).into_bytes_with_nul();
        bytes.zeroize();
    }
}

/// The conversation's function, which PAM's modules call on the lock's
/// thread during `pam_authenticate`.
///
/// # Safety
///
/// PAM calls it with `count` messages at `messages` (an array of pointers
/// on Linux-PAM), a place for the responses, and the `data` given to
/// `pam_start`: a `Talk` this module owns, alive for the whole
/// transaction.
unsafe extern "C" fn converse(
    count: c_int,
    messages: *mut *const Message,
    responses: *mut *mut Response,
    data: *mut c_void,
) -> c_int {
    if count <= 0 || messages.is_null() || responses.is_null() || data.is_null() {
        return PAM_CONV_ERR;
    }
    let count = count as usize;
    // SAFETY: `data` is the `Talk` given to `pam_start`, alive and not
    // aliased while PAM calls back on this thread.
    let talk = unsafe { &mut *data.cast::<Talk>() };
    // SAFETY: PAM frees the responses with `free()` (each `resp`, then the
    // array): they are allocated with the C allocator, zeroed.
    let answers =
        unsafe { libc::calloc(count, std::mem::size_of::<Response>()) }.cast::<Response>();
    if answers.is_null() {
        return PAM_BUF_ERR;
    }
    for i in 0..count {
        // SAFETY: `messages` holds `count` pointers to messages (Linux-PAM's
        // layout), each valid for this call.
        let message = unsafe { &**messages.add(i) };
        match message.style {
            PAM_PROMPT_ECHO_OFF if !talk.asked => {
                talk.asked = true;
                // SAFETY: `password` is a valid C string; `strdup` copies it
                // with the C allocator, which PAM frees after overwriting.
                let copy = unsafe { libc::strdup(talk.password.as_ptr()) };
                if copy.is_null() {
                    // SAFETY: the answers so far, then the array, all from
                    // the C allocator; PAM gets none of them.
                    unsafe { drop_answers(answers, i) };
                    return PAM_BUF_ERR;
                }
                // SAFETY: `answers` has room for `count` responses.
                unsafe { (*answers.add(i)).resp = copy };
            }
            // A second password prompt (a new one asked for, a second
            // factor): not answered here.
            PAM_PROMPT_ECHO_OFF | PAM_PROMPT_ECHO_ON => {
                // SAFETY: as above.
                unsafe { drop_answers(answers, i) };
                return PAM_CONV_ERR;
            }
            PAM_ERROR_MSG | PAM_TEXT_INFO if !message.text.is_null() => {
                // SAFETY: a module's message is a C string, valid for this
                // call.
                let text = unsafe { CStr::from_ptr(message.text) };
                tracing::debug!(said = %text.to_string_lossy(), "PAM says");
            }
            _ => {}
        }
    }
    // SAFETY: `responses` is where PAM takes the array, which it owns now.
    unsafe { *responses = answers };
    PAM_SUCCESS
}

/// Frees the first `given` answers and the array, their texts zeroed.
///
/// # Safety
///
/// `answers` is an array from `calloc` with at least `given` responses,
/// whose `resp` are null or from the C allocator.
unsafe fn drop_answers(answers: *mut Response, given: usize) {
    for i in 0..given {
        // SAFETY: within the array, as the caller says.
        let resp = unsafe { (*answers.add(i)).resp };
        if !resp.is_null() {
            // SAFETY: a C string from `strdup`, zeroed then freed.
            unsafe {
                let length = libc::strlen(resp);
                ptr::write_bytes(resp, 0, length);
                libc::free(resp.cast());
            }
        }
    }
    // SAFETY: the array from `calloc`.
    unsafe { libc::free(answers.cast()) };
}

/// libpam's functions, opened at run time.
struct Pam {
    _library: libloading::Library,
    start: Start,
    authenticate: Step,
    acct_mgmt: Step,
    setcred: Step,
    end: End,
}

impl Pam {
    fn open() -> Result<Self, String> {
        // SAFETY: libpam runs no code of its own as it loads beyond its
        // constructors, which set nothing up that this process relies on.
        let library = unsafe { libloading::Library::new("libpam.so.0") }
            .map_err(|e| format!("libpam: {e}"))?;
        // SAFETY: the types are libpam's own (security/pam_appl.h); the
        // functions stay valid while `_library` is held, which they are
        // kept with.
        unsafe {
            let start = *library
                .get::<Start>(b"pam_start\0")
                .map_err(|e| e.to_string())?;
            let authenticate = *library
                .get::<Step>(b"pam_authenticate\0")
                .map_err(|e| e.to_string())?;
            let acct_mgmt = *library
                .get::<Step>(b"pam_acct_mgmt\0")
                .map_err(|e| e.to_string())?;
            let setcred = *library
                .get::<Step>(b"pam_setcred\0")
                .map_err(|e| e.to_string())?;
            let end = *library
                .get::<End>(b"pam_end\0")
                .map_err(|e| e.to_string())?;
            Ok(Self {
                _library: library,
                start,
                authenticate,
                acct_mgmt,
                setcred,
                end,
            })
        }
    }
}

/// Checks `password` for `user` through PAM's `service`, on this thread:
/// it may take seconds (a module's delay after a failure).
pub fn check(service: &str, user: &str, password: &str) -> Outcome {
    let pam = match Pam::open() {
        Ok(pam) => pam,
        Err(err) => return Outcome::Failed(err),
    };
    let (Ok(service), Ok(user)) = (CString::new(service), CString::new(user)) else {
        return Outcome::Failed("a service or a user with a NUL".to_owned());
    };
    let Ok(password) = CString::new(password) else {
        // A NUL cannot be typed; nothing to give.
        return Outcome::Wrong;
    };
    let mut talk = Box::new(Talk {
        password,
        asked: false,
    });
    let conversation = Conversation {
        converse,
        data: ptr::addr_of_mut!(*talk).cast(),
    };
    let mut handle: *mut c_void = ptr::null_mut();
    // SAFETY: valid C strings and conversation, alive until `pam_end`
    // below; `handle` receives the transaction.
    let started =
        unsafe { (pam.start)(service.as_ptr(), user.as_ptr(), &conversation, &mut handle) };
    if started != PAM_SUCCESS || handle.is_null() {
        return Outcome::Failed(format!("pam_start: {started}"));
    }
    // SAFETY: `handle` is the transaction `pam_start` made, used on this
    // thread only, ended once below.
    let authenticated = unsafe { (pam.authenticate)(handle, 0) };
    let mut status = authenticated;
    if authenticated == PAM_SUCCESS {
        // SAFETY: as above.
        status = unsafe { (pam.acct_mgmt)(handle, 0) };
        if status == PAM_SUCCESS {
            // SAFETY: as above. A failure to refresh credentials (a
            // Kerberos ticket) does not keep the session locked.
            let refreshed = unsafe { (pam.setcred)(handle, PAM_REFRESH_CRED) };
            if refreshed != PAM_SUCCESS {
                tracing::info!(refreshed, "PAM could not refresh the credentials");
            }
        }
    }
    // SAFETY: the transaction ends once; nothing uses `handle` after.
    unsafe { (pam.end)(handle, status) };
    let asked = talk.asked;
    drop(talk);
    match status {
        PAM_SUCCESS => Outcome::Right,
        PAM_MAXTRIES => Outcome::TooMany,
        // Refused before asking for the password: pam_faillock's
        // `preauth`, the account locked for a while.
        PAM_AUTH_ERR | PAM_PERM_DENIED if !asked => Outcome::TooMany,
        PAM_AUTH_ERR => Outcome::Wrong,
        other => {
            tracing::info!(status = other, asked, "PAM refused");
            Outcome::Wrong
        }
    }
}
