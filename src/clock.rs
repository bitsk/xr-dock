// SPDX-License-Identifier: GPL-3.0-or-later
//! Real local wall-clock labels, refreshed only when the minute changes.
pub struct Clock {
    minute: Option<libc::time_t>,
    pub time: String,
    pub date_weekday: String,
}
impl Clock {
    pub fn new() -> Self {
        let mut clock = Self {
            minute: None,
            time: "—".into(),
            date_weekday: "本地时间不可用".into(),
        };
        clock.refresh();
        clock
    }
    pub fn refresh(&mut self) -> bool {
        let now = unsafe { libc::time(std::ptr::null_mut()) };
        if now == -1 || self.minute == Some(now.div_euclid(60)) {
            return false;
        }
        self.minute = Some(now.div_euclid(60));
        let mut local = std::mem::MaybeUninit::<libc::tm>::uninit();
        if unsafe { libc::localtime_r(&now, local.as_mut_ptr()) }.is_null() {
            return false;
        }
        let local = unsafe { local.assume_init() };
        self.time = format!("{:02}:{:02}", local.tm_hour, local.tm_min);
        let weekdays = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
        self.date_weekday = format!(
            "{}月{}日 {}",
            local.tm_mon + 1,
            local.tm_mday,
            weekdays.get(local.tm_wday as usize).copied().unwrap_or("")
        );
        true
    }
}
