#![feature(path_try_exists)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::io::BufRead;
use clap::{app_from_crate, Arg, App};

use std::fs::{OpenOptions, File};

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

fn main() -> Result<(), failure::Error> {
    env_logger::init();

    let m = app_from_crate!()
        .arg(Arg::new("kill").short('k').long("kill"))
        .get_matches();

    let pid_file = PID_FILE;
    let out = std::fs::File::create(LOG_FILE)?;
    let err = std::fs::File::create(ERR_FILE)?;
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


    // for termination signalling
    let term = Arc::new(AtomicBool::new(false));

    log::info!("starting daemon");
    d.start()?;
    log::info!("daemon started");

    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&term))?;
    signal_hook::flag::register(signal_hook::consts::SIGHUP, Arc::clone(&term))?;

    while !term.load(Ordering::Relaxed) {
        // Do some time-limited stuff here
        // (if this could block forever, then there's no guarantee the signal will have any
        // effect).
    }
    std::fs::remove_file(&pid_file).expect("Remove PID file");
    Ok(())
}
