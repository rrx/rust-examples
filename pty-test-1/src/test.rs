use filedescriptor::FileDescriptor;
//use std::io;
use std::os::unix::io::FromRawFd;
//use std::os::unix::io::AsRawFd;
//use std::os::unix::prelude::RawFd;
use std::os::unix::process::CommandExt;
use std::io::{Read, Write};
use std::ptr;
use io_extras::os::rustix::{AsRawFd, AsRawReadWriteFd, AsReadWriteFd, RawFd};
use io_lifetimes::{AsFd, BorrowedFd};
use std::fmt::{self, Debug};
use std::io::IoSlice;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{self, AsyncRead, AsyncWrite, ReadBuf};
use failure::ResultExt;
/// Represents the size of the visible display area in the pty
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde_support", derive(Serialize, Deserialize))]
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
///
/// Represents the master end of a pty.
/// The file descriptor will be closed when the Pty is dropped.
pub struct UnixMasterPty {
    pub fd: PtyFd
}

impl UnixMasterPty {
    fn try_clone_reader(&self) -> Result<Box<dyn Read + Send>, failure::Error> {
        let fd = self.fd.try_clone()?;
        Ok(Box::new(PtyFd::from_fd(fd)?))
    }
}

/// Represents the slave end of a pty.
/// The file descriptor will be closed when the Pty is dropped.
pub struct UnixSlavePty {
    pub fd: PtyFd
}

impl UnixSlavePty {
    pub fn spawn_command(
        &self,
        builder: tokio::process::Command,
    ) -> Result<tokio::process::Child, failure::Error> {
        Ok(self.fd.spawn_command(builder)?)
    }
}

#[derive(Debug)]
pub struct PtyFd {
    pub fd: FileDescriptor,
    pub stream: tokio::net::UnixStream
}

impl std::ops::Deref for PtyFd {
    type Target = FileDescriptor;
    fn deref(&self) -> &FileDescriptor {
        &self.fd
    }
}

impl std::ops::DerefMut for PtyFd {
    fn deref_mut(&mut self) -> &mut FileDescriptor {
        &mut self.fd
    }
}

impl PtyFd {
    pub fn from_fd(fd: FileDescriptor) -> Result<Self, failure::Error> {
        let mut std = unsafe {std::os::unix::net::UnixStream::from_raw_fd(fd.as_raw_fd()) };

        // make sure we are blocking, or pty won't work
        // applications assume blocking, or you will get a Resource Not Available error
        std.set_nonblocking(false)?;

        let mut stream = tokio::net::UnixStream::from_std(std)?;
        Ok(Self { fd, stream })
    }

    pub fn from_raw_fd(raw_fd: RawFd) -> Result<Self, failure::Error> {
        let fd = unsafe {FileDescriptor::from_raw_fd(raw_fd)};
        let mut std = unsafe {std::os::unix::net::UnixStream::from_raw_fd(raw_fd)};

        // make sure we are blocking, or pty won't work
        // applications assume blocking, or you will get a Resource Not Available error
        std.set_nonblocking(false)?;

        let mut stream = tokio::net::UnixStream::from_std(std)?;
        Ok(Self { fd, stream })
    }

    pub fn spawn_command(&self, mut cmd: tokio::process::Command) -> Result<tokio::process::Child, failure::Error> {
        unsafe {
            cmd.pre_exec(move || {
                    // Clean up a few things before we exec the program
                    // Clear out any potentially problematic signal
                    // dispositions that we might have inherited
                    for signo in &[
                        libc::SIGCHLD,
                        libc::SIGHUP,
                        libc::SIGINT,
                        libc::SIGQUIT,
                        libc::SIGTERM,
                        libc::SIGALRM,
                    ] {
                        libc::signal(*signo, libc::SIG_DFL);
                    }

                    // Establish ourselves as a session leader.
                    if libc::setsid() == -1 {
                        log::error!("Unable to set SID: {:?}", io::Error::last_os_error());
                        return Err(io::Error::last_os_error());
                    }

                    // Clippy wants us to explicitly cast TIOCSCTTY using
                    // type::from(), but the size and potentially signedness
                    // are system dependent, which is why we're using `as _`.
                    // Suppress this lint for this section of code.
                    #[cfg_attr(feature = "cargo-clippy", allow(clippy::cast_lossless))]
                    {
                        // Set the pty as the controlling terminal.
                        // Failure to do this means that delivery of
                        // SIGWINCH won't happen when we resize the
                        // terminal, among other undesirable effects.
                        if libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                            log::error!("Unable to set TTY: {:?}", io::Error::last_os_error());
                            return Err(io::Error::last_os_error());
                        }
                    }

                    close_random_fds();

                    //if let Some(mask) = configured_umask {
                        //libc::umask(mask);
                    //}

                    Ok(())
                })
        };

        let mut child = cmd.spawn()?;

        // Ensure that we close out the slave fds that Child retains;
        // they are not what we need (we need the master side to reference
        // them) and won't work in the usual way anyway.
        // In practice these are None, but it seems best to be move them
        // out in case the behavior of Command changes in the future.
        child.stdin.take();
        child.stdout.take();
        child.stderr.take();

        Ok(child)
    }

}

impl Read for PtyFd {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
        match self.fd.read(buf) {
            Err(ref e) if e.raw_os_error() == Some(libc::EIO) => {
                // EIO indicates that the slave pty has been closed.
                // Treat this as EOF so that std::io::Read::read_to_string
                // and similar functions gracefully terminate when they
                // encounter this condition
                Ok(0)
            }
            x => x,
        }
    }
}

impl AsyncRead for PtyFd {
    #[inline]
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for PtyFd {
    #[inline]
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write_vectored(cx, bufs)
    }

    #[inline]
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    #[inline]
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

/// Helper function to set the close-on-exec flag for a raw descriptor
fn cloexec(fd: RawFd) -> Result<(), io::Error> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags == -1 {
        log::error!( "fcntl to read flags failed: {:?}", io::Error::last_os_error());
        return Err(io::Error::last_os_error());
    }
    let result = unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) };
    if result == -1 {
        log::error!("fcntl to set CLOEXEC failed: {:?}", io::Error::last_os_error());
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn openpty(size: PtySize) -> Result<(UnixMasterPty, UnixSlavePty), failure::Error> {
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
        //bail!("failed to openpty: {:?}", io::Error::last_os_error());
    }

    let master = UnixMasterPty {
        fd: PtyFd::from_raw_fd(master)?
    };
    let slave = UnixSlavePty {
        fd: PtyFd::from_raw_fd(slave)?
    };

    // Ensure that these descriptors will get closed when we execute
    // the child process.  This is done after constructing the Pty
    // instances so that we ensure that the Ptys get drop()'d if
    // the cloexec() functions fail (unlikely!).
    cloexec(master.fd.as_raw_fd())?;
    cloexec(slave.fd.as_raw_fd())?;

    Ok((master, slave))

}

/// On Big Sur, Cocoa leaks various file descriptors to child processes,
/// so we need to make a pass through the open descriptors beyond just the
/// stdio descriptors and close them all out.
/// This is approximately equivalent to the darwin `posix_spawnattr_setflags`
/// option POSIX_SPAWN_CLOEXEC_DEFAULT which is used as a bit of a cheat
/// on macOS.
/// On Linux, gnome/mutter leak shell extension fds to wezterm too, so we
/// also need to make an effort to clean up the mess.
///
/// This function enumerates the open filedescriptors in the current process
/// and then will forcibly call close(2) on each open fd that is numbered
/// 3 or higher, effectively closing all descriptors except for the stdio
/// streams.
///
/// The implementation of this function relies on `/dev/fd` being available
/// to provide the list of open fds.  Any errors in enumerating or closing
/// the fds are silently ignored.
pub fn close_random_fds() {
    // FreeBSD, macOS and presumably other BSDish systems have /dev/fd as
    // a directory listing the current fd numbers for the process.
    //
    // On Linux, /dev/fd is a symlink to /proc/self/fd
    if let Ok(dir) = std::fs::read_dir("/dev/fd") {
        let mut fds = vec![];
        for entry in dir {
            if let Some(num) = entry
                .ok()
                .map(|e| e.file_name())
                .and_then(|s| s.into_string().ok())
                .and_then(|n| n.parse::<libc::c_int>().ok())
            {
                if num > 2 {
                    fds.push(num);
                }
            }
        }
        for fd in fds {
            unsafe {
                libc::close(fd);
            }
        }
    }
}


