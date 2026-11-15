use crate::model::Result;

#[cfg(windows)]
mod platform {
    use super::Result;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        },
    };

    pub struct ProcessTree(HANDLE);
    // A job handle is owned, closed once, and can be used from any thread.
    unsafe impl Send for ProcessTree {}

    impl ProcessTree {
        pub fn new() -> Result<Self> {
            unsafe {
                let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if handle.is_null() {
                    return Err(std::io::Error::last_os_error().to_string());
                }
                let tree = Self(handle);
                let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                if SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    std::mem::size_of_val(&limits) as u32,
                ) == 0
                {
                    return Err(std::io::Error::last_os_error().to_string());
                }
                Ok(tree)
            }
        }

        pub fn attach(&self, process: HANDLE) -> Result<()> {
            if unsafe { AssignProcessToJobObject(self.0, process) } == 0 {
                return Err(format!(
                    "Could not contain process tree: {}",
                    std::io::Error::last_os_error()
                ));
            }
            Ok(())
        }
    }

    impl Drop for ProcessTree {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}
#[cfg(windows)]
pub use platform::ProcessTree;

#[cfg(not(windows))]
pub struct ProcessTree;
#[cfg(not(windows))]
impl ProcessTree {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }
}
