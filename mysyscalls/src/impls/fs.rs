use crate::LinuxError;
use alloc::string::String;
use alloc::sync::Arc;
use axfs::api::{File, OpenOptions};
use axio::{Read, Write};
use mycore::fs::FileDesc;
use spin::Mutex;

const O_RDONLY: usize = 0o0;
const O_WRONLY: usize = 0o1;
const O_RDWR: usize = 0o2;
const O_CREAT: usize = 0o100;
const O_TRUNC: usize = 0o1000;
const O_APPEND: usize = 0o2000;

fn read_user_string(addr: usize) -> Option<String> {
    if addr == 0 {
        return None;
    }

    let mut s = String::new();
    let mut ptr = addr as *const u8;

    unsafe {
        loop {
            let c = *ptr;
            if c == 0 {
                break;
            }
            s.push(c as char);
            ptr = ptr.add(1);

            if s.len() > 4096 {
                return None;
            }
        }
    }

    Some(s)
}

pub fn sys_read(fd: usize, buf: usize, count: usize) -> isize {
    axlog::debug!("sys_read: fd={}, buf={:#x}, count={}", fd, buf, count);

    if buf == 0 || count == 0 {
        return 0;
    }

    if fd == 0 {
        let slice = unsafe { core::slice::from_raw_parts_mut(buf as *mut u8, count) };
        let mut i = 0;
        while i < count {
            let mut c = [0u8; 1];
            let n = axhal::console::read_bytes(&mut c);
            if n == 0 {
                if i > 0 {
                    break;
                }
                axtask::yield_now();
                continue;
            }
            slice[i] = c[0];
            i += 1;
            if c[0] == b'\n' || c[0] == b'\r' {
                break;
            }
        }
        return i as isize;
    }

    let proc = match mycore::task::current_process() {
        Some(p) => p,
        None => {
            axlog::error!("sys_read: no current process");
            return -LinuxError::ESRCH.code() as isize;
        }
    };

    let fd_table = proc.fd_table.lock();
    let file_desc = match fd_table.get(fd) {
        Some(desc) => desc,
        None => {
            axlog::error!("sys_read: invalid fd {}", fd);
            return -LinuxError::EBADF.code() as isize;
        }
    };

    match file_desc {
        FileDesc::Stdio => {
            axlog::error!("sys_read: trying to read from stdout/stderr");
            -LinuxError::EBADF.code() as isize
        }
        FileDesc::File(file) => {
            let slice = unsafe { core::slice::from_raw_parts_mut(buf as *mut u8, count) };
            let mut file = file.lock();

            match file.read(slice) {
                Ok(n) => {
                    axlog::debug!("sys_read: read {} bytes from fd {}", n, fd);
                    n as isize
                }
                Err(e) => {
                    axlog::error!("sys_read: failed to read from fd {}: {:?}", fd, e);
                    -LinuxError::EIO.code() as isize
                }
            }
        }
    }
}

pub fn sys_write(fd: usize, buf: usize, count: usize) -> isize {
    axlog::debug!("sys_write: fd={}, buf={:#x}, count={}", fd, buf, count);

    if fd == 1 || fd == 2 {
        if buf == 0 || count == 0 {
            return 0;
        }

        let slice = unsafe { core::slice::from_raw_parts(buf as *const u8, count) };

        if let Ok(s) = core::str::from_utf8(slice) {
            axlog::ax_print!("{}", s);
        } else {
            for &byte in slice {
                axlog::ax_print!("{}", byte as char);
            }
        }

        return count as isize;
    }

    let proc = match mycore::task::current_process() {
        Some(p) => p,
        None => return -LinuxError::ESRCH.code() as isize,
    };

    let fd_table = proc.fd_table.lock();
    let file_desc = match fd_table.get(fd) {
        Some(desc) => desc,
        None => return -LinuxError::EBADF.code() as isize,
    };

    match file_desc {
        FileDesc::Stdio => -LinuxError::EBADF.code() as isize,
        FileDesc::File(file) => {
            let slice = unsafe { core::slice::from_raw_parts(buf as *const u8, count) };
            let mut file = file.lock();

            match file.write(slice) {
                Ok(n) => n as isize,
                Err(_) => -LinuxError::EIO.code() as isize,
            }
        }
    }
}

pub fn sys_openat(dirfd: i32, pathname: usize, flags: usize, mode: usize) -> isize {
    let path = match read_user_string(pathname) {
        Some(s) => s,
        None => {
            axlog::error!("sys_openat: invalid pathname pointer");
            return -LinuxError::EFAULT.code() as isize;
        }
    };

    axlog::debug!(
        "sys_openat: dirfd={}, pathname='{}', flags={:#x}, mode={:#o}",
        dirfd,
        path,
        flags,
        mode
    );

    let access_mode = flags & 0o3;
    let mut opts = OpenOptions::new();

    match access_mode {
        O_RDONLY => {
            opts.read(true);
        }
        O_WRONLY => {
            opts.write(true);
        }
        O_RDWR => {
            opts.read(true).write(true);
        }
        _ => {
            axlog::error!("sys_openat: invalid access mode {}", access_mode);
            return -LinuxError::EINVAL.code() as isize;
        }
    }

    if flags & O_CREAT != 0 {
        opts.create(true);
    }
    if flags & O_TRUNC != 0 {
        opts.truncate(true);
    }
    if flags & O_APPEND != 0 {
        opts.append(true);
    }

    let file = match opts.open(&path) {
        Ok(f) => f,
        Err(e) => {
            axlog::error!("sys_openat: failed to open '{}': {:?}", path, e);
            return -LinuxError::ENOENT.code() as isize;
        }
    };

    let proc = match mycore::task::current_process() {
        Some(p) => p,
        None => {
            axlog::error!("sys_openat: no current process");
            return -LinuxError::ESRCH.code() as isize;
        }
    };

    let mut fd_table = proc.fd_table.lock();
    let fd = match fd_table.alloc_fd(FileDesc::File(Arc::new(Mutex::new(file)))) {
        Some(fd) => fd,
        None => {
            axlog::error!("sys_openat: failed to allocate fd");
            return -LinuxError::EMFILE.code() as isize;
        }
    };

    axlog::debug!("sys_openat: opened '{}' as fd {}", path, fd);
    fd as isize
}

pub fn sys_close(fd: usize) -> isize {
    axlog::debug!("sys_close: fd={}", fd);

    let proc = match mycore::task::current_process() {
        Some(p) => p,
        None => return -LinuxError::ESRCH.code() as isize,
    };

    let mut fd_table = proc.fd_table.lock();
    if fd_table.close(fd) {
        axlog::debug!("sys_close: closed fd {}", fd);
        0
    } else {
        axlog::error!("sys_close: invalid fd {}", fd);
        -LinuxError::EBADF.code() as isize
    }
}

pub fn sys_fstat(fd: usize, statbuf: usize) -> isize {
    axlog::debug!("sys_fstat: fd={}, statbuf={:#x}", fd, statbuf);

    if statbuf == 0 {
        return -LinuxError::EFAULT.code() as isize;
    }

    let proc = match mycore::task::current_process() {
        Some(p) => p,
        None => return -LinuxError::ESRCH.code() as isize,
    };

    let fd_table = proc.fd_table.lock();
    let file_desc = match fd_table.get(fd) {
        Some(desc) => desc,
        None => return -LinuxError::EBADF.code() as isize,
    };

    match file_desc {
        FileDesc::Stdio => {
            // TODO
            let stat = unsafe { &mut *(statbuf as *mut Stat) };
            *stat = Stat::default();
            stat.st_mode = 0o020000 | 0o666;
            stat.st_rdev = if fd == 0 { 0x8800 } else { 0x8801 };
            0
        }
        FileDesc::File(file) => {
            let file = file.lock();
            match file.metadata() {
                Ok(metadata) => {
                    let stat = unsafe { &mut *(statbuf as *mut Stat) };
                    *stat = Stat::default();

                    if metadata.is_file() {
                        stat.st_mode = 0o100000 | 0o644;
                    } else if metadata.is_dir() {
                        stat.st_mode = 0o040000 | 0o755;
                    }

                    stat.st_size = metadata.len() as i64;
                    stat.st_blocks = metadata.blocks() as i64;
                    stat.st_blksize = 4096;

                    axlog::debug!("sys_fstat: fd {} size={}", fd, stat.st_size);
                    0
                }
                Err(e) => {
                    axlog::error!("sys_fstat: failed to get metadata for fd {}: {:?}", fd, e);
                    -LinuxError::EIO.code() as isize
                }
            }
        }
    }
}

/// Linux stat 简化结构体
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Stat {
    pub st_dev: u64,
    pub st_ino: u64,
    pub st_mode: u32,
    pub st_nlink: u32,
    pub st_uid: u32,
    pub st_gid: u32,
    pub st_rdev: u64,
    pub __pad1: u64,
    pub st_size: i64,
    pub st_blksize: i32,
    pub __pad2: i32,
    pub st_blocks: i64,
    pub st_atime: i64,
    pub st_atime_nsec: i64,
    pub st_mtime: i64,
    pub st_mtime_nsec: i64,
    pub st_ctime: i64,
    pub st_ctime_nsec: i64,
    pub __unused: [i32; 2],
}

impl Default for Stat {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Iovec {
    iov_base: usize,
    iov_len: usize,
}

pub fn sys_writev(fd: usize, iov: usize, iovcnt: usize) -> isize {
    axlog::debug!("sys_writev: fd={}, iov={:#x}, iovcnt={}", fd, iov, iovcnt);

    if iov == 0 || iovcnt == 0 {
        return 0;
    }
    if iovcnt > 1024 {
        return -LinuxError::EINVAL.code() as isize;
    }

    let iov_slice = unsafe { core::slice::from_raw_parts(iov as *const Iovec, iovcnt) };
    let mut total = 0isize;

    for iov_entry in iov_slice {
        if iov_entry.iov_base == 0 || iov_entry.iov_len == 0 {
            continue;
        }
        let ret = sys_write(fd, iov_entry.iov_base, iov_entry.iov_len);
        if ret < 0 {
            return if total > 0 { total } else { ret };
        }
        total += ret;
    }

    total
}
