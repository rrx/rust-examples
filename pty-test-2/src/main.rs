use std::io;
use std::os::unix::prelude::RawFd;
use std::os::unix::io::FromRawFd;
use std::os::unix::io::AsRawFd;
use std::ptr;
use filedescriptor::FileDescriptor;
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use pty_test_2::*;


fn md5sum() -> Result<(), failure::Error> {
    let h = duct::cmd!("md5sum")
        .stdin_bytes(&b"asdf\n"[..])
        .start()?;
    let pids = h.pids();
    println!("pids {:?}", pids);
    let o = h.wait()?;
    println!("{:?}", o);
    Ok(())
}

fn cat() -> Result<(), failure::Error> {
    let h = duct::cmd!("cat")
        .stdin_bytes(&b"asdf\n"[..])
        .start()?;
    let pids = h.pids();
    println!("pids {:?}", pids);
    let o = h.wait()?;
    println!("{:?}", o);
    Ok(())
}

fn do_pty(program: String) -> Result<(), failure::Error> {
    log::info!("p: {}", program);
    let size = PtySize::default();
    let (master, slave) = openpty(size)?;

    let e = duct::cmd!(program)
        .stdin_bytes(&b"asdf\n"[..])
        .stdin_file(slave.try_clone()?)
        .stdout_file(slave.try_clone()?)
        .stderr_file(slave)
        .unchecked()
        .before_spawn(|cmd| {
            unsafe {
                // Establish ourselves as a session leader.
                //if libc::setsid() == -1 {
                    //log::error!("Unable to set SID: {:?}", io::Error::last_os_error());
                    //return Err(io::Error::last_os_error());
                //}

                //if libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                    //log::error!("Unable to set TTY: {:?}", io::Error::last_os_error());
                    //return Err(io::Error::last_os_error().into());
                //}
            }
            Ok(())
        });

    let mut h = e.start()?;//reader()?;

    let pids = h.pids();
    println!("pids {:?}", pids);

    use std::io::Read;
    let mut f = unsafe { std::fs::File::from_raw_fd(master.as_raw_fd()) };
    //f.set_nonblocking(true);
    let mut buffer = [0;100];
    let mut s = String::new();
    loop {
        //let r = f.read_to_string(&mut s)?;
        //println!("{:?}", r);
        let n = f.read(&mut buffer)?;
        println!("{:?}", (n, &buffer[..n]));

        match h.try_wait() {
            Ok(Some(o)) => {
                log::info!("child status: {:?}", (o.status.success(), o.status.code(), o.status));
                break;
            }
            Ok(None) => {
                println!("continue");
                continue;
            }
            _ => break
        }
    }

    //let mut b = std::io::Read::bytes(&mut f);
    //println!("{:?}", buffer);
    //let o = h.wait()?;
    //log::info!("child status: {:?}", (o.status.success(), o.status.code(), o.status));
    //let mut b = std::io::Read::bytes(&mut f);
    //println!("{:?}", buffer);

    //let mut s = String::new();
    //loop {
        //let r = h.read_to_string(&mut s)?;
        //if r > 0 {
            //println!("{:?}", r);

            //if s.len() > 0 {
                //println!("{:?}", s);
            //}
        //}
        ////match b.next() {
            ////Some(v) => println!("{:?}", v),
            ////None => break
        ////}
        ////match std::io::Read::read(&mut f, &mut buffer[..]) {
            ////Ok(r) => println!("r{:?}", (r, &buffer[..r])),
            ////Err(e) => break
        ////}
    //}

    //let o = h.wait()?;
    //println!("{:?}", (o.status.success(), o.status.code()));

    Ok(())
}

fn test_mio(program: String) -> Result<(), failure::Error> {
    log::info!("mio: {}", program);
    let size = PtySize::default();
    let (mut master, slave) = openpty(size)?;

    let e = duct::cmd!(program)
        //.stdin_bytes(&b"asdf\n"[..])
        .stdin_file(slave.try_clone()?)
        .stdout_file(slave.try_clone()?)
        .stderr_file(slave)
        .unchecked();

    let mut h = e.start()?;

    let pids = h.pids();
    println!("pids {:?}", pids);
    use mio::{Events, Token, Poll, Interest};

    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(1024);


    poll.registry().register(
        &mut mio::unix::SourceFd(&master.as_raw_fd()),
        Token(0), Interest::READABLE | Interest::WRITABLE)?;

    poll.poll(&mut events, Some(std::time::Duration::from_millis(100)))?;
    use std::io::{Write, Read};

    //writeln!(master, "hello")?;

    let mut buf = String::new();
    'outer: loop {
        for x in events.iter() {
            match x.token() {
                Token(0) if x.is_readable() => {
                    master.read_to_string(&mut buf)?;
                    println!("read {:?}", (buf));
                }
                _ => {
                    println!("{:?}", (x.token(), x.is_readable(), x.is_writable()));
                }
            }

            println!("event {:?}", (x.token()));
            match h.try_wait() {
                Ok(Some(o)) => {
                    println!("o {:?}", (o));
                    break 'outer;
                }
                Ok(None) => {
                }
                Err(e) => {
                    println!("e {:?}", (e));
                    break 'outer;
                }
            }

        }
    }

    let o = h.wait()?;
    println!("exit {:?}", (o.status.success(), o.status.code(), o));
    
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtySize {
    /// The number of lines of text
    pub rows: u16,
    /// The number of columns of text
    pub cols: u16,
    /// The width of a cell in pixels.  Note that some systems never
    /// fill this value and ignore it.
    pub pixel_width: u16,
    /// The height of a cell in pixels.  Note that some systems never
    /// fill this value and ignore it.
    pub pixel_height: u16,
}

impl Default for PtySize {
    fn default() -> Self {
        PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

pub fn openpty(size: PtySize) -> Result<(FileDescriptor, FileDescriptor), failure::Error> {
    let mut master: RawFd = -1;
    let mut slave: RawFd = -1;

    let mut size = libc::winsize {
        ws_row: size.rows,
        ws_col: size.cols,
        ws_xpixel: size.pixel_width,
        ws_ypixel: size.pixel_height,
    };

    let result = unsafe {
        // BSDish systems may require mut pointers to some args
        #[cfg_attr(feature = "cargo-clippy", allow(clippy::unnecessary_mut_passed))]
        libc::openpty(
            &mut master,
            &mut slave,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut size,
        )
    };

    log::info!("open: {:?}", (master, slave));

    if result != 0 {
        log::error!("Failed to openpty: {:?}", io::Error::last_os_error());
        return Err(io::Error::last_os_error().into());
    }

    let master = unsafe {FileDescriptor::from_raw_fd(master)};
    let slave = unsafe {FileDescriptor::from_raw_fd(slave)};

    Ok((master, slave))
}

fn main() -> Result<(), failure::Error> {
    env_logger::init();
    md5sum()?;
    cat()?;
    test_mio("ls".into())?;
    do_pty("tty".into())?;//, &mut vec![])?;
    //do_pty("top".into())?;//, &mut vec![])?;
    //do_pty("cat".into())?;//, &mut vec![])?;
    Ok(())
}
