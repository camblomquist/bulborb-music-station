use std::time::Duration;

#[allow(dead_code)]
pub struct Clock {
    ppq: u16,
    pulse: Duration,
    latency: Duration,
}

impl Clock {
    pub fn new(tempo: u32, ppq: u16) -> Self {
        let micros: u64 = (tempo as f64 / ppq as f64).floor() as u64;
        let pulse = Duration::from_micros(micros);
        Clock {
            ppq,
            pulse,
            latency: Duration::ZERO,
        }
    }

    #[allow(dead_code)]
    pub fn set_tempo(&mut self, tempo: u32) {
        let micros: u64 = (tempo as f64 / self.ppq as f64).floor() as u64;
        self.pulse = Duration::from_micros(micros);
    }

    pub fn pulse_time(&self) -> Duration {
        self.pulse
    }

    pub fn tick(&mut self, last_cycle: Duration) {
        // Account for time of previous cycle in determining next pulse
        let Some(mut dur) = self.pulse.checked_sub(last_cycle) else {
            eprintln!("Clock can't keep up!");
            return;
        };
        // Can't guarantee sleep for less than 1ms
        // Determine total submillisecond latency then remove from sleep time
        // Will sleep for 1 additional ms every few ticks to balance out
        dur += self.latency;
        let submillis_micros = dur.subsec_micros() as u64 - dur.subsec_millis() as u64 * 1000;
        self.latency = Duration::from_micros(submillis_micros);
        dur -= self.latency;

        Self::sleep(dur);
    }

    fn sleep(dur: Duration) {
        std::thread::sleep(dur);
    }
}
