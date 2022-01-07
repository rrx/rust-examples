#![feature(path_try_exists)]
#![feature(hash_drain_filter)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::io::BufRead;
use clap::{app_from_crate, Arg};
use std::rc::Rc;
use std::sync::Mutex;
use std::pin::Pin;
use std::os::unix::net::UnixStream;
use std::fs::{OpenOptions, File};
use std::borrow::BorrowMut;

const PID_FILE: &str = "/tmp/service.pid";
const LOG_FILE: &str = "/tmp/service.log";
const ERR_FILE: &str = "/tmp/service.err";


fn kill_daemon(pid_file: &str) {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;
    use nix::pty::SessionId;

    if !std::fs::try_exists(pid_file).expect("PID file not available") {
        return;
    }

    if let Ok(f) = OpenOptions::new().read(true).open(pid_file) {
        std::io::BufReader::new(f).lines()
            .take(1)
            .filter_map(|line| {
                match line {
                    Ok(v) => v.trim().parse().ok(),
                    _ => None
                }
            })
            .for_each(|v: SessionId| {
                log::info!("Killing {}", v);
                if let Err(e) = kill(Pid::from_raw(v), Signal::SIGHUP) {
                    log::error!("{:?}", e);
                }
            });
    }

    std::fs::remove_file(&pid_file);//.expect("Remove PID file2");
}


struct Process {
    child: std::process::Child,
    stdin: UnixStream,
    stdout: UnixStream,
    stderr: UnixStream,
    id: ulid::Ulid
}

impl Process {
    fn new(cmd: &str) -> Result<Self, failure::Error> {
        let mut s = shlex::split(cmd).ok_or(failure::err_msg("Unable to parse command"))?;
        if s.len() == 0 {
            return Err(failure::err_msg("Invalid command"));
        }
        let args = s.split_off(1);

        let (stdin_a, stdin_b) = UnixStream::pair().unwrap();
        let (stdout_a, stdout_b) = UnixStream::pair().unwrap();
        let (stderr_a, stderr_b) = UnixStream::pair().unwrap();

        let mut child = std::process::Command::new("sleep").args(vec!["1"])
            //.stdin(stdin_a.as_stdio())
            //.stdout(stdout_a)
            //.stderr(stderr_a)
            .spawn()?;

        let id = ulid::Ulid::new();
        Ok(Process { id, child, stdin: stdin_b, stdout: stdout_b, stderr: stderr_b })
    }
}

fn main() -> Result<(), failure::Error> {
    env_logger::init();

    let m = app_from_crate!()
        .arg(Arg::new("kill").short('k').long("kill"))
        .arg(Arg::new("foreground").short('f').long("foreground"))
        .get_matches();

    let pid_file = PID_FILE;
    let out = File::create(LOG_FILE)?;
    let err = File::create(ERR_FILE)?;

    let foreground = m.is_present("foreground");
    if !foreground {
        let d = daemonize::Daemonize::new()
            .pid_file(pid_file)
            .stdout(out)
            .stderr(err);

        if m.is_present("kill") {
            kill_daemon(pid_file);
            return Ok(());
        }

        if std::fs::try_exists(pid_file).expect("PID file not available") {
            log::error!("PID file exists. Service may be running");
            std::process::exit(1);
        }
        log::info!("starting daemon");
        d.start()?;
    }
    log::info!("service started");

    // for termination signalling
    let term = Arc::new(AtomicBool::new(false));

    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&term))?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))?;
    signal_hook::flag::register(signal_hook::consts::SIGHUP, Arc::clone(&term))?;

    let mut children = std::collections::HashMap::new();

    for i in 0..100 {
        let p = Process::new("sleep 1").unwrap();
        children.insert(p.id, p);
    }

    while !term.load(Ordering::Relaxed) {
        let mut c = &mut children;
        c.drain_filter(|id, p| {
            match p.child.try_wait() {
                Ok(Some(status)) => {
                    log::info!("child returned: {:?}", (status.success(), status.code()));
                    true
                }
                Ok(None) => {
                    //wait
                    false
                }
                Err(e) => {
                    log::error!("wait: {:?}", e);
                    false
                }
            }
        });
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    // on exit, send kill, then wait to reap the child
    children.iter_mut().for_each(|(id, p)| {
        log::info!("kill {}", id);
        p.child.kill();
    });

    children.iter_mut().for_each(|(id, p)| {
        log::info!("wait {}", id);
        p.child.wait();
    });

    children.drain_filter(|id, child| {
        true
    });

    if !foreground {
        std::fs::remove_file(&pid_file).expect("Remove PID file");
    }

    log::info!("service stopped");
    Ok(())
}
