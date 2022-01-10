use std::ffi::CString;
use std::io::Read;
use std::process::{Command};
use std::io::BufRead;
use pty::fork::*;
use std::io::Write;

fn main_master(mut master: Master) {
    // Read output via PTY master
    let mut output = String::new();
    let mut buffer = [0; 10];
    master.write(b"asdf\n");
    master.write(b"fdsa");
    master.flush();
    loop {
        let r = master.read(&mut buffer).unwrap();
        if r == 0 {
            break;
        }

        if let Ok(v) = utf8::decode(&buffer[..r]) {
            print!("{}", v);
        } else {
            print!("a{:?}", &buffer[..r]);
        }
    }
}

fn main_child() {
  let mut args = std::env::args().collect::<Vec<String>>();
  if args.len() < 1 {
      return;
  }
  let mut args = args.split_off(1);
  let s_args = args.split_off(1);
  let s_command = args.get(0).unwrap();
  let mut command = Command::new(s_command);
  command.args(s_args);
  command.status().expect("could not execute tty");
}

fn main() {
  let fork = Fork::from_ptmx().unwrap();
  if let Some(mut master) = fork.is_parent().ok() {
      main_master(master);
  }
  else {
      main_child();
  }
}
