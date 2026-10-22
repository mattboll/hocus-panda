//! Password checks for the lock screen, through PAM (Linux-PAM and
//! OpenPAM share the API), on a worker thread: a check can take seconds
//! (a slow module, a delay after a failure) and never runs on the loop.
//! Tests give the service a checker of their own.

use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    path::Path,
    sync::mpsc,
};

/// What checks a password.
pub trait Checker: Send + 'static {
    fn check(&mut self, user: &str, password: &str) -> bool;
}

/// The password service: requests go to its thread, results come back
/// through `done`.
#[derive(Debug)]
pub struct Auth {
    requests: mpsc::Sender<(String, String)>,
}

impl Auth {
    /// Starts the thread; `done` is called from it with each result.
    pub fn start(
        mut checker: Box<dyn Checker>,
        done: impl Fn(bool) + Send + 'static,
    ) -> std::io::Result<Self> {
        let (requests, queue) = mpsc::channel::<(String, String)>();
        std::thread::Builder::new()
            .name("auth".into())
            .spawn(move || {
                while let Ok((user, mut password)) = queue.recv() {
                    let ok = checker.check(&user, &password);
                    // Not left lying in memory longer than needed.
                    wipe(&mut password);
                    done(ok);
                }
            })?;
        Ok(Self { requests })
    }

    pub fn check(&self, user: &str, password: String) {
        let _ = self.requests.send((user.to_owned(), password));
    }
}

fn wipe(s: &mut String) {
    let mut bytes = std::mem::take(s).into_bytes();
    bytes.iter_mut().for_each(|b| *b = 0);
    std::hint::black_box(&bytes);
}

/// PAM, with the `hocuspanda` service when installed, else `login`.
#[derive(Debug, Default)]
pub struct Pam;

impl Pam {
    fn service() -> &'static str {
        let ours = ["/etc/pam.d/hocuspanda", "/usr/local/etc/pam.d/hocuspanda"];
        if ours.iter().any(|p| Path::new(p).exists()) {
            "hocuspanda"
        } else {
            "login"
        }
    }
}

impl Checker for Pam {
    fn check(&mut self, user: &str, password: &str) -> bool {
        match pam_check(Self::service(), user, password) {
            Ok(()) => true,
            Err(code) => {
                tracing::info!(code, "the password was refused");
                false
            }
        }
    }
}

// The PAM application API, as Linux-PAM and OpenPAM define it.

const PAM_SUCCESS: c_int = 0;
const PAM_CONV_ERR: c_int = 19;
const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_PROMPT_ECHO_ON: c_int = 2;

#[repr(C)]
struct PamMessage {
    msg_style: c_int,
    msg: *const c_char,
}

#[repr(C)]
struct PamResponse {
    resp: *mut c_char,
    resp_retcode: c_int,
}

#[repr(C)]
struct PamConv {
    conv: extern "C" fn(c_int, *mut *const PamMessage, *mut *mut PamResponse, *mut c_void) -> c_int,
    appdata_ptr: *mut c_void,
}

// SAFETY: declarations of libpam's C API, used only in the blocks below.
#[allow(unsafe_code)]
#[link(name = "pam")]
unsafe extern "C" {
    fn pam_start(
        service: *const c_char,
        user: *const c_char,
        conv: *const PamConv,
        pamh: *mut *mut c_void,
    ) -> c_int;
    fn pam_authenticate(pamh: *mut c_void, flags: c_int) -> c_int;
    fn pam_acct_mgmt(pamh: *mut c_void, flags: c_int) -> c_int;
    fn pam_end(pamh: *mut c_void, status: c_int) -> c_int;
}

// SAFETY: declarations of the C library's allocation functions, which PAM
// expects its responses to come from.
#[allow(unsafe_code)]
unsafe extern "C" {
    fn calloc(n: usize, size: usize) -> *mut c_void;
    fn strdup(s: *const c_char) -> *mut c_char;
}

/// Answers PAM's prompts with the password; other messages get no answer.
extern "C" fn conversation(
    count: c_int,
    messages: *mut *const PamMessage,
    responses: *mut *mut PamResponse,
    data: *mut c_void,
) -> c_int {
    if count <= 0 || messages.is_null() || responses.is_null() || data.is_null() {
        return PAM_CONV_ERR;
    }
    let n = count as usize;
    // SAFETY: PAM frees the responses with free(): they are allocated with
    // calloc, zeroed, `n` of them as PAM asked.
    #[allow(unsafe_code)]
    let replies = unsafe { calloc(n, std::mem::size_of::<PamResponse>()) }.cast::<PamResponse>();
    if replies.is_null() {
        return PAM_CONV_ERR;
    }
    // SAFETY: `data` is the password's CString given to pam_start below,
    // alive for the whole PAM transaction.
    #[allow(unsafe_code)]
    let password = unsafe { &*data.cast::<CString>() };
    for i in 0..n {
        // SAFETY: PAM hands `count` message pointers (Linux-PAM and OpenPAM
        // agree on an array of pointers); each is valid during the call.
        #[allow(unsafe_code)]
        let message = unsafe { &**messages.add(i) };
        if matches!(message.msg_style, PAM_PROMPT_ECHO_OFF | PAM_PROMPT_ECHO_ON) {
            // SAFETY: `replies` holds `n` zeroed responses; strdup copies the
            // password into memory PAM frees.
            #[allow(unsafe_code)]
            unsafe {
                (*replies.add(i)).resp = strdup(password.as_ptr());
            }
        }
    }
    // SAFETY: `responses` is PAM's out pointer for the array.
    #[allow(unsafe_code)]
    unsafe {
        *responses = replies;
    }
    PAM_SUCCESS
}

fn pam_check(service: &str, user: &str, password: &str) -> Result<(), c_int> {
    let service = CString::new(service).map_err(|_| PAM_CONV_ERR)?;
    let user = CString::new(user).map_err(|_| PAM_CONV_ERR)?;
    let password = CString::new(password).map_err(|_| PAM_CONV_ERR)?;
    transaction(service.as_c_str(), user.as_c_str(), &password)
}

/// One PAM transaction: authenticate, then check the account.
fn transaction(service: &CStr, user: &CStr, password: &CString) -> Result<(), c_int> {
    let conv = PamConv {
        conv: conversation,
        appdata_ptr: std::ptr::from_ref(password).cast_mut().cast(),
    };
    let mut handle: *mut c_void = std::ptr::null_mut();
    // SAFETY: the strings and the conversation outlive the transaction,
    // which ends with pam_end below whatever happens.
    #[allow(unsafe_code)]
    let started = unsafe { pam_start(service.as_ptr(), user.as_ptr(), &conv, &mut handle) };
    if started != PAM_SUCCESS || handle.is_null() {
        return Err(started);
    }
    // SAFETY: `handle` comes from a successful pam_start.
    #[allow(unsafe_code)]
    let result = unsafe {
        match pam_authenticate(handle, 0) {
            PAM_SUCCESS => pam_acct_mgmt(handle, 0),
            failed => failed,
        }
    };
    // SAFETY: as above; the handle is not used after.
    #[allow(unsafe_code)]
    unsafe {
        pam_end(handle, result);
    }
    if result == PAM_SUCCESS {
        Ok(())
    } else {
        Err(result)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    struct Fixed(&'static str);

    impl Checker for Fixed {
        fn check(&mut self, _: &str, password: &str) -> bool {
            password == self.0
        }
    }

    #[test]
    fn checks_go_through_the_thread() {
        let results = Arc::new(Mutex::new(Vec::new()));
        let auth = Auth::start(Box::new(Fixed("panda")), {
            let results = results.clone();
            move |ok| results.lock().expect("results").push(ok)
        })
        .expect("started");
        auth.check("alex", "bamboo".into());
        auth.check("alex", "panda".into());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while results.lock().expect("results").len() < 2 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(*results.lock().expect("results"), vec![false, true]);
    }

    /// Reads the machine's PAM configuration and leaves a failed attempt
    /// in its logs: run by hand (`cargo test -- --ignored`).
    #[test]
    #[ignore = "uses the machine's PAM"]
    fn pam_refuses_a_wrong_password() {
        // A user that does not exist: refused without asking anyone.
        assert!(!Pam.check("hocuspanda-nobody", "x"));
    }
}
