use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Mutex;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct TokenBucket {
    capacity: f64,
    per_sec: f64,
    tokens: f64,
    updated: Instant,
}

impl TokenBucket {
    pub fn new(capacity: u32, per_sec: f64) -> Self {
        Self {
            capacity: f64::from(capacity),
            per_sec,
            tokens: f64::from(capacity),
            updated: Instant::now(),
        }
    }

    pub fn try_take(&mut self, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.updated).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.per_sec).min(self.capacity);
        self.updated = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    fn is_full_at(&self, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.updated).as_secs_f64();
        self.tokens + elapsed * self.per_sec >= self.capacity
    }
}

/// One token bucket per key (e.g. client IP), shared across requests.
pub struct KeyedLimiter<K> {
    template: TokenBucket,
    buckets: Mutex<HashMap<K, TokenBucket>>,
}

const SWEEP_THRESHOLD: usize = 10_000;

impl<K: Eq + Hash> KeyedLimiter<K> {
    /// Allows `per_minute` requests per minute on average, with bursts up to the same amount.
    pub fn per_minute(per_minute: u32) -> Self {
        Self {
            template: TokenBucket::new(per_minute, f64::from(per_minute) / 60.0),
            buckets: Mutex::new(HashMap::new()),
        }
    }

    pub fn check(&self, key: K) -> bool {
        let now = Instant::now();
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        if buckets.len() >= SWEEP_THRESHOLD {
            // A full bucket behaves exactly like a fresh one, so it can be forgotten.
            buckets.retain(|_, b| !b.is_full_at(now));
        }
        buckets
            .entry(key)
            .or_insert_with(|| TokenBucket {
                updated: now,
                ..self.template.clone()
            })
            .try_take(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn bucket_refills_over_time() {
        let start = Instant::now();
        let mut bucket = TokenBucket {
            updated: start,
            ..TokenBucket::new(2, 1.0)
        };
        assert!(bucket.try_take(start));
        assert!(bucket.try_take(start));
        assert!(!bucket.try_take(start));
        assert!(bucket.try_take(start + Duration::from_millis(1_000)));
    }
}
