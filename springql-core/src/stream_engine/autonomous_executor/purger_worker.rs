// This file is part of https://github.com/SpringQL/SpringQL which is licensed under MIT OR Apache-2.0. See file LICENSE-MIT or LICENSE-APACHE for full license details.

pub(in crate::stream_engine::autonomous_executor) mod purger_worker_thread;

use std::sync::Arc;

use crate::stream_engine::autonomous_executor::{
    event_queue::EventQueue,
    worker::worker_handle::{WorkerHandle, WorkerStopCoordinator},
};

use self::purger_worker_thread::{PurgerWorkerThread, PurgerWorkerThreadArg};

use super::worker::worker_handle::WorkerSetupCoordinator;

/// Worker to execute pump and sink tasks.
#[derive(Debug)]
pub(super) struct PurgerWorker {
    _handle: WorkerHandle,
}

impl PurgerWorker {
    pub(super) fn new(
        event_queue: Arc<EventQueue>,
        worker_setup_coordinator: Arc<WorkerSetupCoordinator>,
        worker_stop_coordinator: Arc<WorkerStopCoordinator>,
        thread_arg: PurgerWorkerThreadArg,
    ) -> Self {
        let handle = WorkerHandle::new::<PurgerWorkerThread>(
            event_queue,
            worker_setup_coordinator,
            worker_stop_coordinator,
            thread_arg,
        );
        Self { _handle: handle }
    }
}
