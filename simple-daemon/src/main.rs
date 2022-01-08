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
use std::os::unix::io::FromRawFd;
use std::io::{BufReader, LineWriter};
use std::os::unix::io::AsRawFd;
use std::collections::HashMap;

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

#[derive(Debug)]
enum WaitStatus {
    Exited(Option<i32>),
    Signaled,
    Alive
}

impl Into<WaitStatus> for std::process::ExitStatus {
    fn into(self) -> WaitStatus {
        match self.code() {
            Some(c) => WaitStatus::Exited(Some(c as i32)),
            None => WaitStatus::Signaled,
        }
    }
}

impl Into<WaitStatus> for rexpect::process::wait::WaitStatus {
    fn into(self) -> WaitStatus {
        use rexpect::process::wait;
        match self {
            wait::WaitStatus::Exited(_, code) => WaitStatus::Exited(Some(code as i32)),
            wait::WaitStatus::StillAlive => WaitStatus::Alive,
            wait::WaitStatus::Signaled(_,_,_) => WaitStatus::Signaled,
            _ => WaitStatus::Exited(None)
        }
    }
}


struct StdProcess {
    command: String,
    child: std::process::Child,
    stdin: UnixStream,
    stdout: UnixStream,
    stderr: UnixStream,
    id: ulid::Ulid
}

struct ExpectProcess {
    id: ulid::Ulid,
    command: String,
    child: rexpect::process::PtyProcess
}

trait Process {
    fn try_wait(&mut self) -> Option<WaitStatus>;
    fn kill(&mut self) -> std::io::Result<()>;
    fn wait(&mut self) -> std::io::Result<WaitStatus>;
    fn get_command(&self) -> &str;
}

impl ExpectProcess {
    fn new(cmd: &str) -> Result<Self, failure::Error> {
        let mut s = shlex::split(cmd).ok_or(failure::err_msg("Unable to parse command"))?;
        if s.len() == 0 {
            return Err(failure::err_msg("Invalid command"));
        }
        let args = s.split_off(1);
        let s_command = s.get(0).unwrap();
        let mut command = std::process::Command::new(s_command);
        command.args(args);
        let mut child = rexpect::process::PtyProcess::new(command).expect("unable to execute");
        let fd = nix::unistd::dup(child.pty.as_raw_fd()).unwrap();
        let f = unsafe { File::from_raw_fd(fd) };
        let mut writer = LineWriter::new(&f);
        let mut reader = BufReader::new(&f);
        let id = ulid::Ulid::new();
        Ok(ExpectProcess { command: String::from(cmd), child, id })
    }
}

impl Process for ExpectProcess {
    fn try_wait(&mut self) -> Option<WaitStatus> {
        use rexpect::process::wait;
        match self.child.status() {
            Some(w) => Some(w.into()),
            None => None
        }
    }
    fn wait(&mut self) -> std::io::Result<WaitStatus> {
        if let Ok(w) = self.child.wait() {
            return Ok(w.into())
        }
        panic!();
    }
    fn kill(&mut self) -> std::io::Result<()> {
        match self.child.signal(rexpect::process::signal::Signal::SIGTERM) {
            Ok(()) => Ok(()),
            Err(e) => panic!()
        }
    }
    fn get_command(&self) -> &str {
        self.command.as_str()
    }
}

impl StdProcess {
    fn new_std(cmd: &str) -> Result<Self, failure::Error> {
        let mut s = shlex::split(cmd).ok_or(failure::err_msg("Unable to parse command"))?;
        if s.len() == 0 {
            return Err(failure::err_msg("Invalid command"));
        }
        let args = s.split_off(1);
        let s_command = s.get(0).unwrap();

        let (stdin_a, stdin_b) = UnixStream::pair().unwrap();
        let (stdout_a, stdout_b) = UnixStream::pair().unwrap();
        let (stderr_a, stderr_b) = UnixStream::pair().unwrap();

        let mut child = std::process::Command::new(s_command).args(args)
            //.stdin(stdin_a.as_stdio())
            //.stdout(stdout_a)
            //.stderr(stderr_a)
            .spawn()?;

        let id = ulid::Ulid::new();
        Ok(StdProcess { command: String::from(cmd), id, child, stdin: stdin_b, stdout: stdout_b, stderr: stderr_b })
    }
}

impl Process for StdProcess {
    fn try_wait(&mut self) -> Option<WaitStatus> {
        use std::process::ExitStatus;
        match self.child.try_wait() {
            Ok(Some(e)) => Some(e.into()),
            Ok(None) => Some(WaitStatus::Alive), 
            Err(e) => None
        }
    }

    fn wait(&mut self) -> std::io::Result<WaitStatus> {
        match self.child.wait() {
            Ok(w) => Ok(w.into()),
            Err(e) => panic!()
        }
    }

    fn kill(&mut self) -> std::io::Result<()> {
        self.child.kill()
    }

    fn get_command(&self) -> &str {
        self.command.as_str()
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

    let mut children: HashMap<ulid::Ulid, Box<dyn Process>> = HashMap::new();

    for i in 0..20 {
        let p = StdProcess::new_std("sleep 6").unwrap();
        children.insert(p.id, Box::new(p));
    }

    for i in 0..20 {
        let p = ExpectProcess::new("sleep 5").unwrap();
        children.insert(p.id, Box::new(p));
    }

    while !term.load(Ordering::Relaxed) {
        let mut c = &mut children;
        c.drain_filter(|id, p| {
            match p.try_wait() {
                Some(WaitStatus::Exited(Some(code))) => {
                    log::info!("child returned: {:?}: {}", code, p.get_command());
                    true
                }
                Some(WaitStatus::Signaled) => {
                    log::info!("child returned signaled: {}", p.get_command());
                    true
                }
                Some(WaitStatus::Alive) => {
                    false
                }
                Some(WaitStatus::Exited(None)) => {
                    log::info!("child returned None {}", p.get_command());
                    true
                }
                None => {
                    //wait
                    false
                }
            }
        });
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    // on exit, send kill, then wait to reap the child
    children.iter_mut().for_each(|(id, p)| {
        log::info!("kill {}", id);
        p.kill().unwrap();
    });

    children.iter_mut().for_each(|(id, p)| {
        log::info!("wait {}, {}", id, p.get_command());
        p.wait().unwrap();
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
