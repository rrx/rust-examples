use rexpect::process::PtyProcess;
use std::process::Command;
use std::fs::File;
use std::io::{BufReader, LineWriter};
use std::os::unix::io::{FromRawFd, AsRawFd};
use nix::unistd::dup;
use std::io::BufRead;
use mio::unix::SourceFd;
use std::os::unix::io::RawFd;
use mio::{Poll, Token, Registry, Events, Interest};
use std::io::Read;
pub struct MyIo {
    fd: RawFd,
}

//impl event::Source for MyIo {
    //fn register(&mut self, registry: &Registry, token: Token, interests: Interest)
        //-> io::Result<()>
    //{
        //SourceFd(&self.fd).register(registry, token, interests)
    //}

    //fn reregister(&mut self, registry: &Registry, token: Token, interests: Interest)
        //-> io::Result<()>
    //{
        //SourceFd(&self.fd).reregister(registry, token, interests)
    //}

    //fn deregister(&mut self, registry: &Registry) -> io::Result<()> {
        //SourceFd(&self.fd).deregister(registry)
    //}
//}


// this works 
fn read() {
    let mut process = PtyProcess::new(Command::new("tty")).expect("could not execute cat");
    let fd = dup(process.pty.as_raw_fd()).unwrap();
    let f = unsafe { File::from_raw_fd(fd) };
    let mut writer = LineWriter::new(&f);
    let mut reader = BufReader::new(&f);
    let mut line = String::new();
    let r = reader.read_line(&mut line).unwrap();
    println!("{}", line);
    process.exit().expect("could not terminate process");
}

fn main() -> std::io::Result<()> { //,failure::Error> {
    let mut process = PtyProcess::new(Command::new("top")).expect("could not execute cat");
    let fd = dup(process.pty.as_raw_fd()).unwrap();
    //let f = MyIo { fd };
    let f = unsafe { File::from_raw_fd(fd) };

    let poll = mio::Poll::new()?;

    let events = Events::with_capacity(128);
    // Register the listener
    poll.registry().register(
        &mut SourceFd(&fd),
        Token(0),
        Interest::READABLE)?;
    let mut reader = BufReader::new(&f);
    //
    // Process each event.
    let mut line = String::new();
    //let r = reader.read_line(&mut line).unwrap();
    //print!("a:{}", line);
    let mut buffer = [0; 10];

    use rexpect::process::wait::WaitStatus::*;


    loop {
        if !events.is_empty() {
            // We can use the token we previously provided to `register` to
            // determine for which type the event is.
            match events.iter().next().unwrap().token() {
                Token(0) => {
                    let r = reader.read(&mut buffer).unwrap();
                    println!("a:{}", line);
                    //let r = reader.read_line(&mut line).unwrap();
                    //println!("out:{}", line);
                }
                _ => ()
            }
        }

        if let Some(e) = process.status() {
            match e {
                Exited(_, c) => {
                    println!("exit: {:?}", c);
                    let r = reader.read(&mut buffer).unwrap();
                    println!("a:{}", line);
                    break;
                }
                Signaled(_, s, _) => {
                    println!("signal: {:?}", s);
                    break;
                }
                _ => ()
            }
        }
    }
    
    if !events.is_empty() {
        match events.iter().next().unwrap().token() {
            Token(0) => {
                let r = reader.read(&mut buffer).unwrap();
                println!("a:{}", line);
                //let r = reader.read_line(&mut line).unwrap();
                //println!("a:{}", line);
            }
            _ => ()
        }
    }

    process.exit().expect("could not terminate process");
    Ok(())
}

