use std::sync::{Mutex, MutexGuard};

use zg_engine::{CoreEvent, EmitError, EventSink};

#[derive(Debug, Default)]
pub struct RecordedEvents {
    events: Mutex<Vec<CoreEvent>>,
}

impl RecordedEvents {
    #[must_use]
    pub fn events(&self) -> Vec<CoreEvent> {
        self.lock_events().clone()
    }

    fn lock_events(&self) -> MutexGuard<'_, Vec<CoreEvent>> {
        match self.events.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

impl EventSink for RecordedEvents {
    fn try_emit(&self, event: CoreEvent) -> Result<(), EmitError> {
        self.lock_events().push(event);
        Ok(())
    }
}
