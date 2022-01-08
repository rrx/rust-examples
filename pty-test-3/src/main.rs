use std::ffi::CString;
use std::io::Read;
use std::process::{Command};
use std::io::BufRead;
use pty::fork::*;

fn main() {
  let fork = Fork::from_ptmx().unwrap();

  if let Some(mut master) = fork.is_parent().ok() {
    // Read output via PTY master
    let mut output = String::new();
    let mut buffer = [0; 10];
    loop {
        let r = master.read(&mut buffer).unwrap();
        if r == 0 {
            break;
        }
        //println!("a:{:?}", (r, &buffer[..r]));
        if let Ok(v) = utf8::decode(&buffer[..r]) {
            print!("{}", v);
        }

        //match master.read_to_string(&mut output) {
          //Ok(_nread) => println!("child tty is: {}", output.trim()),
          //Err(e)     => panic!("read error: {}", e),
        //}
    }

  }
  else {
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
}
