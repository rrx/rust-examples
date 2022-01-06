
//! A utility library that adds asynchronous support to file-like objects on
//! Unix-like platforms.
//!
//! This crate is primarily intended for pipes and other files that support
//! nonblocking I/O.  Regular files do not support nonblocking I/O, so this
//! crate has no effect on them.
//!
//! See [`File`](struct.File.html) for an example of how a file can be made
//! suitable for asynchronous I/O.
use mio::unix::SourceFd;
use mio::{Interest, Token, Events, Registry, event};

use std::cell::RefCell;
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::{fs, io};
use tokio::io::Ready;
use tokio::io::unix::AsyncFd;
use std::pin::Pin;
use std::task::{Context};
use std::marker::Unpin;
 use tokio::io::ReadBuf;
use futures::io::IoSlice;
use tokio::io::{AsyncRead, AsyncWrite};
use futures::StreamExt;

unsafe fn dupe_file_from_fd(old_fd: RawFd) -> io::Result<fs::File> {
    let fd = libc::fcntl(old_fd, libc::F_DUPFD_CLOEXEC, 0);
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(fs::File::from_raw_fd(fd))
}

/// Duplicate the standard input file.
///
/// Unlike `std::io::Stdin`, this file is not buffered.
pub fn raw_stdin() -> io::Result<fs::File> {
    unsafe { dupe_file_from_fd(libc::STDIN_FILENO) }
}

/// Duplicate the standard output file.
///
/// Unlike `std::io::Stdout`, this file is not buffered.
pub fn raw_stdout() -> io::Result<fs::File> {
    unsafe { dupe_file_from_fd(libc::STDOUT_FILENO) }
}

/// Duplicate the standard error file.
///
/// Unlike `std::io::Stderr`, this file is not buffered.
pub fn raw_stderr() -> io::Result<fs::File> {
    unsafe { dupe_file_from_fd(libc::STDERR_FILENO) }
}

/// Gets the nonblocking mode of the underlying file descriptor.
///
/// Implementation detail: uses `fcntl` to retrieve `O_NONBLOCK`.
pub fn get_nonblocking<F: AsRawFd>(file: &F) -> io::Result<bool> {
    unsafe {
        let flags = libc::fcntl(file.as_raw_fd(), libc::F_GETFL);
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(flags & libc::O_NONBLOCK != 0)
    }
}

/// Sets the nonblocking mode of the underlying file descriptor to either on
/// (`true`) or off (`false`).  If `File::new_nb` was previously used to
/// construct the `File`, then nonblocking mode has already been turned on.
///
/// This function is not atomic. It should only called if you have exclusive
/// control of the underlying file descriptor.
///
/// Implementation detail: uses `fcntl` to query the flags and set
/// `O_NONBLOCK`.
pub fn set_nonblocking<F: AsRawFd>(file: &mut F, nonblocking: bool) -> io::Result<()> {
    unsafe {
        let fd = file.as_raw_fd();
        // shamelessly copied from libstd/sys/unix/fd.rs
        let previous = libc::fcntl(fd, libc::F_GETFL);
        if previous < 0 {
            return Err(io::Error::last_os_error());
        }
        let new = if nonblocking {
            previous | libc::O_NONBLOCK
        } else {
            previous & !libc::O_NONBLOCK
        };
        if libc::fcntl(fd, libc::F_SETFL, new) < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

/// Wraps file-like objects for asynchronous I/O.
///
/// Normally, you should use `File::new_nb` rather than `File::raw_new` unless
/// the underlying file descriptor has already been set to nonblocking mode.
/// Using a file descriptor that is not in nonblocking mode for asynchronous
/// I/O will lead to subtle and confusing bugs.
///
/// Wrapping regular files has no effect because they do not support
/// nonblocking mode.
///
/// The most common instantiation of this type is `File<std::fs::File>`, which
/// indirectly provides the following trait implementation:
///
/// ## Example: unsafe creation from raw file descriptor
///
/// To unsafely create `File<F>` from a raw file descriptor `fd`, you can do
/// something like:
///
/// which will enable nonblocking mode upon creation.  The choice of `F` is
/// critical: it determines the ownership semantics of the file descriptor.
/// For example, if you choose `F = std::fs::File`, the file descriptor will
/// be closed when the `File` is dropped.
#[derive(Debug)]
pub struct File<F: AsRawFd> {
    file: F,
    //stream: AsyncFd<F>
    //evented: RefCell<Option<mio::Registration>>,
}

impl<F: AsRawFd> File<F> {
    /// Wraps a file-like object into a pollable object that supports
    /// `tokio::io::AsyncRead` and `tokio::io::AsyncWrite`, and also *enables
    /// nonblocking mode* on the underlying file descriptor.
    pub fn new_nb(mut file: F) -> io::Result<Self> {
        set_nonblocking(&mut file, true)?;
        File::raw_new(file)
    }

    /// Raw constructor that **does not enable nonblocking mode** on the
    /// underlying file descriptor.  This constructor should only be used if
    /// you are certain that the underlying file descriptor is already in
    /// nonblocking mode.
    pub fn raw_new(file: F) -> io::Result<Self> {
        Ok(Self { file: file }) //, stream: AsyncFd::new(file)? })
    }
}

impl<F: AsRawFd> AsRawFd for File<F> {
    fn as_raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
}

impl<F: AsRawFd> mio::event::Source for File<F> {
    fn register(&mut self, registry: &Registry, token: Token, interests: Interest)
        -> io::Result<()>
    {
        SourceFd(&self.as_raw_fd()).register(registry, token, interests)
    }

    fn reregister(&mut self, registry: &Registry, token: Token, interests: Interest)
        -> io::Result<()>
    {
        SourceFd(&self.as_raw_fd()).reregister(registry, token, interests)
    }

    fn deregister(&mut self, registry: &Registry) -> io::Result<()> {
        SourceFd(&self.as_raw_fd()).deregister(registry)
    }
}


//impl<F: AsRawFd> mio::event::Source for File<F> {
    //fn register(
        //&mut self,
        //registry: &mio::Registry,
        //token: mio::Token,
        //interest: mio::Interest,
    //) -> io::Result<()> {
        //match mio::unix::SourceFd(&self.as_raw_fd()).register(registry, token, interest) {
            //// this is a workaround for regular files, which are not supported
            //// by epoll; they would instead cause EPERM upon registration
            //Err(e) => e,
            ////Err(ref e) if e.raw_os_error() == Some(libc::EPERM) => {
                ////set_nonblocking(&mut self.as_raw_fd(), false)?;
                ////// workaround: PollEvented/IoToken always starts off in the
                ////// "not ready" state so we have to use a real Evented object
                ////// to set its readiness state
                ////let (r, s) = mio::Registration::new2();
                ////r.register(poll, token, interest)?;
                ////s.set_readiness(Ready::readable() | Ready::writable())?;
                ///[>self.evented.borrow_mut() = Some(r);
                ////Ok(())
            ////}
            //e => e,
        //}
    //}

impl<F: io::Read + AsRawFd> io::Read for File<F> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.file.read(buf) {
            Err(ref e) if e.raw_os_error() == Some(libc::EIO) => {
                // EIO indicates that the slave pty has been closed.
                // Treat this as EOF so that std::io::Read::read_to_string
                // and similar functions gracefully terminate when they
                // encounter this condition
                Ok(0)
            }
            x => x,
        }
        //self.file.read(buf)
    }
}

impl<F: io::Write + AsRawFd> io::Write for File<F> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl<F: io::Seek + AsRawFd> io::Seek for File<F> {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        self.file.seek(pos)
    }
}

impl <F: AsyncRead + Unpin + AsRawFd> AsyncRead for File<F> {
    #[inline]
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf,
    ) -> std::task::Poll<io::Result<()>> {
        Pin::new(&mut self.file).poll_read(cx, buf)
    }
}

impl<F: AsyncWrite + Unpin + AsRawFd> AsyncWrite for File<F> {
    #[inline]
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<io::Result<usize>> {
        Pin::new(&mut self.file).poll_write(cx, buf)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> std::task::Poll<io::Result<usize>> {
        Pin::new(&mut self.file).poll_write_vectored(cx, bufs)
    }

    #[inline]
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> std::task::Poll<io::Result<()>> {
        Pin::new(&mut self.file).poll_flush(cx)
    }

    #[inline]
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> std::task::Poll<io::Result<()>> {
        Pin::new(&mut self.file).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    #[test]
    fn test_nonblocking() -> io::Result<()> {
        let (sock, _) = UnixStream::pair()?;
        let mut fd = sock.as_raw_fd();
        set_nonblocking(&mut fd, false)?;
        assert!(!get_nonblocking(&fd)?);
        set_nonblocking(&mut fd, true)?;
        assert!(get_nonblocking(&fd)?);
        set_nonblocking(&mut fd, false)?;
        assert!(!get_nonblocking(&fd)?);
        Ok(())
    }
}
