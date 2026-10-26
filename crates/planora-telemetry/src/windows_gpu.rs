//! Per-instance GPU Engine counters. No system GPU total is inferred from these values.
use planora_core::{GpuEngineSample, Metric};
use std::{
    ptr,
    time::{Duration, Instant},
};
use windows_sys::Win32::System::Performance::{
    PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE,
    PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA, PdhAddEnglishCounterW, PdhCloseQuery,
    PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
};

pub struct WindowsGpu {
    query: usize,
    counter: usize,
    last: Option<Instant>,
    cached: Vec<GpuEngineSample>,
}

impl WindowsGpu {
    pub fn new() -> Result<Self, String> {
        let mut query: PDH_HQUERY = ptr::null_mut();
        let status = unsafe { PdhOpenQueryW(ptr::null(), 0, &mut query) };
        if status != 0 {
            return Err(format!("PdhOpenQueryW: 0x{status:08x}"));
        }
        let mut counter: PDH_HCOUNTER = ptr::null_mut();
        let path: Vec<u16> = "\\GPU Engine(*)\\Utilization Percentage"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let status = unsafe { PdhAddEnglishCounterW(query, path.as_ptr(), 0, &mut counter) };
        if status != 0 {
            unsafe {
                PdhCloseQuery(query);
            }
            return Err(format!("GPU Engine counter unavailable: 0x{status:08x}"));
        }
        let _ = unsafe { PdhCollectQueryData(query) }; // first rate sample only primes PDH
        Ok(Self {
            query: query as usize,
            counter: counter as usize,
            last: None,
            cached: Vec::new(),
        })
    }

    pub fn sample(&mut self, stamp: u64) -> Result<Vec<GpuEngineSample>, String> {
        if self
            .last
            .is_some_and(|last| last.elapsed() < Duration::from_secs(1))
        {
            return Ok(self.cached.clone());
        }
        let status = unsafe { PdhCollectQueryData(self.query as PDH_HQUERY) };
        if status != 0 {
            return Err(format!("PdhCollectQueryData: 0x{status:08x}"));
        }
        self.last = Some(Instant::now());
        let mut size = 0u32;
        let mut count = 0u32;
        let counter = self.counter as PDH_HCOUNTER;
        let status = unsafe {
            PdhGetFormattedCounterArrayW(
                counter,
                PDH_FMT_DOUBLE,
                &mut size,
                &mut count,
                ptr::null_mut(),
            )
        };
        if status != PDH_MORE_DATA {
            if status == 0 {
                self.cached.clear();
                return Ok(Vec::new());
            }
            return Err(format!("GPU Engine query: 0x{status:08x}"));
        }
        if size > 8 * 1024 * 1024 {
            return Err("GPU Engine counter set exceeds 8 MiB".into());
        }
        let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
        let status = unsafe {
            PdhGetFormattedCounterArrayW(
                counter,
                PDH_FMT_DOUBLE,
                &mut size,
                &mut count,
                buffer.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>(),
            )
        };
        if status != 0 {
            return Err(format!("GPU Engine values: 0x{status:08x}"));
        }
        if count > 4_096 {
            return Err("GPU Engine instance count exceeds 4096".into());
        }
        let mut samples = Vec::with_capacity(count as usize);
        let items = unsafe {
            std::slice::from_raw_parts(
                buffer.as_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>(),
                count as usize,
            )
        };
        for item in items {
            if item.FmtValue.CStatus != PDH_CSTATUS_VALID_DATA
                && item.FmtValue.CStatus != PDH_CSTATUS_NEW_DATA
            {
                continue;
            }
            let value = unsafe { item.FmtValue.Anonymous.doubleValue };
            if !value.is_finite() || !(0.0..=100.0).contains(&value) || item.szName.is_null() {
                continue;
            }
            let mut len = 0usize;
            while len < 1024 && unsafe { *item.szName.add(len) } != 0 {
                len += 1;
            }
            if len == 1024 {
                continue;
            }
            let name =
                String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(item.szName, len) });
            let engine_type = name
                .split("_engtype_")
                .nth(1)
                .unwrap_or("Unknown")
                .to_owned();
            samples.push(GpuEngineSample {
                instance: name,
                engine_type,
                utilization_percent: Metric::available(
                    value as f32,
                    "%",
                    "Windows PDH GPU Engine",
                    stamp,
                ),
            });
        }
        self.cached = samples.clone();
        Ok(samples)
    }
}

impl Drop for WindowsGpu {
    fn drop(&mut self) {
        unsafe {
            PdhCloseQuery(self.query as PDH_HQUERY);
        }
    }
}

pub fn counter_available() -> bool {
    WindowsGpu::new().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_counter_instances_have_valid_percentages() {
        let Ok(mut provider) = WindowsGpu::new() else {
            return;
        };
        std::thread::sleep(Duration::from_millis(1_100));
        let samples = provider
            .sample(crate::unix_ms())
            .expect("installed GPU Engine counter should be readable");
        assert!(samples.len() <= 4_096);
        for sample in samples {
            assert!(!sample.instance.is_empty());
            assert!(
                sample
                    .utilization_percent
                    .value
                    .is_some_and(|v| (0.0..=100.0).contains(&v))
            );
        }
    }
}
