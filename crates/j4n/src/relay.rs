//! `j4n relay -- PROGRAM ARGS…`: runs a program, here a remote shell of a lane, and hands on
//! what it printed with every secret in it replaced by its placeholder, so the lane's model never sees the value. Protecting
//! secrets is one of J4n's main promises, and an `ssh` line that greps a token out of a server's env file returns the token straight to the model.
//!
//! What it does: the program's standard output and standard error are each read to the end (capped), then scrubbed as a model's view of a config file is
//! (`Redactor::view`: a bare `token`, `key`, `password` name counts and so does an unquoted value, a password in a `scheme://user:pass@host` URL, a PEM block, a
//! vendor key shape), then printed, and the exit status is the program's. The registered secrets of this machine (the keys J4n keeps and the logins in its
//! environment) are replaced exactly as well. It prints nothing before the program has ended: the secret in a PEM block or a multi-line value is known only whole.
//!
//! What it does not do: it sees the program's output and nothing else. A remote program that sends the secret somewhere else (a request to a server, a file on the
//! remote machine, a file copied back by `scp`), a value in a form no detector knows (base64 of a secret, a split value), and a secret that is short or has an
//! ordinary look under an ordinary name are out of its reach. The Gate asks about a line whose output goes to a file, a background job or a copied file.

use j4n_core::redact::Redactor;
use std::io::{Read, Write};
use std::process::{Command, Stdio};

/// How much of each stream is read; the rest is read and thrown away so the program is never blocked on a full pipe.
const CAP: u64 = 16 << 20;

/// What a model may read of `bytes` the program printed: text goes through the detector as a config file's text does, and bytes that are not text lose the registered
/// values only.
pub fn view(red: &Redactor, bytes: &[u8]) -> Vec<u8> {
    match std::str::from_utf8(bytes) {
        Ok(text) => red.view(text, "remote.env").0.into_bytes(),
        Err(_) => red.scrub_bytes(bytes),
    }
}

/// The redactor this machine's secrets are registered in: the logins in the environment and the keys J4n keeps.
fn redactor() -> Redactor {
    let red = Redactor::new();
    for (name, value) in std::env::vars().filter(|(n, _)| j4n_core::host::key_like(n)) {
        red.register(&name, &value);
    }
    if let Ok(setup) = crate::wire::Setup::from_env() {
        // a key that cannot be read is no reason to print the rest unscrubbed: the detector still runs
        drop(j4n_core::keys::Keys::new(&setup.home).register_all(&red));
    }
    red
}

fn read_all(stream: Option<impl Read + Send + 'static>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut s) = stream {
            drop(s.by_ref().take(CAP).read_to_end(&mut buf));
            drop(std::io::copy(&mut s, &mut std::io::sink()));
        }
        buf
    })
}

/// Run `argv`; its output goes out scrubbed and its status is ours. 2 when there is nothing to run, 127 when it cannot be started.
pub fn run(argv: &[String]) -> i32 {
    let Some((program, args)) = argv.split_first() else {
        eprintln!("j4n relay: nothing to run (`j4n relay -- PROGRAM ARGS…`)");
        return 2;
    };
    let red = redactor();
    let mut cmd = Command::new(program);
    j4n_connect::proc::scrub(&mut cmd);
    let mut child = match cmd.args(args).stdin(Stdio::inherit()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("j4n relay: could not run `{program}`: {e}");
            return 127;
        }
    };
    let (out, err) = (read_all(child.stdout.take()), read_all(child.stderr.take()));
    let status = child.wait();
    let (out, err) = (out.join().unwrap_or_default(), err.join().unwrap_or_default());
    // a program that ended on a signal has no code: 128 + the signal, as a shell reports it
    let code = status.map_or(1, |s| {
        use std::os::unix::process::ExitStatusExt;
        s.code().or_else(|| s.signal().map(|n| 128 + n)).unwrap_or(1)
    });
    drop(std::io::stdout().write_all(&view(&red, &out)));
    drop(std::io::stderr().write_all(&view(&red, &err)));
    drop(std::io::stdout().flush());
    code
}
