// This file is part of https://github.com/SpringQL/SpringQL which is licensed under MIT OR Apache-2.0. See file LICENSE-MIT or LICENSE-APACHE for full license details.

#![doc = include_str!("pipeline.md")]

pub(crate) mod field;
pub(crate) mod name;
pub(crate) mod option;
pub(crate) mod pipeline_graph;
pub(crate) mod pipeline_version;
pub(crate) mod pump_model;
pub(crate) mod relation;
pub(crate) mod sink_writer_model;
pub(crate) mod source_reader_model;
pub(crate) mod stream_model;

#[cfg(test)]
pub(crate) mod test_support;

use std::{
    collections::HashSet,
    sync::{Arc, Once},
    thread,
    time::Duration,
};

use anyhow::anyhow;

use crate::{
    api::{
        error::{Result, SpringError},
        SpringConfig,
    },
    pipeline::{
        name::{QueueName, StreamName},
        pipeline_graph::PipelineGraph,
        pipeline_version::PipelineVersion,
        pump_model::PumpModel,
        sink_writer_model::SinkWriterModel,
        source_reader_model::SourceReaderModel,
        stream_model::StreamModel,
    },
    sql_processor::SqlProcessor,
    stream_engine::{command::Command, EngineMutex, SinkRow},
};

fn setup_logger() {
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        let _ = env_logger::builder()
            .is_test(false) // To enable color. Logs are not captured by test framework.
            .try_init();
        log_panics::init();
    });

    log::info!("setup_logger(): done");
}
/// Connection object.
///
/// 1 stream pipeline has only 1 connection.
/// In other words, the lifecycle of SpringConnection and internal stream pipeline are the same.
#[derive(Debug)]
pub struct SpringPipeline {
    engine: EngineMutex,
    sql_processor: SqlProcessor,
}

/// Row object from an in memory queue.
#[derive(Debug)]
pub struct SpringRow(pub(crate) SinkRow);

impl From<SinkRow> for SpringRow {
    fn from(sink_row: SinkRow) -> Self {
        Self(sink_row)
    }
}

/// Creates and open an in-process stream pipeline.
pub fn spring_open(config: &SpringConfig) -> Result<SpringPipeline> {
    setup_logger();

    let engine = EngineMutex::new(config);
    let sql_processor = SqlProcessor::default();

    Ok(SpringPipeline {
        engine,
        sql_processor,
    })
}

/// Execute commands (DDL).
///
/// # Failure
///
/// - `SpringError::Sql` when:
///   - Invalid SQL syntax.
///   - Refers to undefined objects (streams, pumps, etc)
///   - Other semantic errors.
/// - `SpringError::InvalidOption` when:
///   - `OPTIONS` in `CREATE` statement includes invalid key or value.
pub fn spring_command(pipeline: &SpringPipeline, sql: &str) -> Result<()> {
    let mut engine = pipeline.engine.get()?;

    let command = pipeline
        .sql_processor
        .compile(sql, engine.current_pipeline())?;

    match command {
        Command::AlterPipeline(c) => engine.alter_pipeline(c),
    }
}

/// Pop a row from an in memory queue. This is a blocking function.
///
/// **Do not call this function from threads.**
/// If you need to pop from multiple in-memory queues using threads, use `spring_pop_non_blocking()`.
/// See: <https://github.com/SpringQL/SpringQL/issues/125>
///
/// # Failure
///
/// - `SpringError::Unavailable` when:
///   - queue named `queue` does not exist.
pub fn spring_pop(pipeline: &SpringPipeline, queue: &str) -> Result<SpringRow> {
    const SLEEP_MSECS: u64 = 10;

    let mut engine = pipeline.engine.get()?;

    loop {
        if let Some(sink_row) =
            engine.pop_in_memory_queue_non_blocking(QueueName::new(queue.to_string()))?
        {
            return Ok(SpringRow::from(sink_row));
        } else {
            thread::sleep(Duration::from_millis(SLEEP_MSECS));
        }
    }
}

/// Pop a row from an in memory queue. This is a non-blocking function.
///
/// # Returns
///
/// - `Ok(Some)` when at least a row is in the queue.
/// - `None` when no row is in the queue.
///
/// # Failure
///
/// - `SpringError::Unavailable` when:
///   - queue named `queue` does not exist.
pub fn spring_pop_non_blocking(
    pipeline: &SpringPipeline,
    queue: &str,
) -> Result<Option<SpringRow>> {
    let mut engine = pipeline.engine.get()?;
    let sink_row = engine.pop_in_memory_queue_non_blocking(QueueName::new(queue.to_string()))?;
    Ok(sink_row.map(SpringRow::from))
}

#[derive(Clone, Debug)]
pub(crate) struct Pipeline {
    version: PipelineVersion,
    object_names: HashSet<String>,
    graph: PipelineGraph,
}

impl Pipeline {
    pub(super) fn new(version: PipelineVersion) -> Self {
        Self {
            version,
            object_names: HashSet::default(),
            graph: PipelineGraph::default(),
        }
    }

    pub(super) fn version(&self) -> PipelineVersion {
        self.version
    }

    pub(super) fn as_graph(&self) -> &PipelineGraph {
        &self.graph
    }

    /// # Failure
    ///
    /// - `SpringError::Sql` when:
    ///   - Stream is not registered in pipeline
    pub(super) fn get_stream(&self, stream: &StreamName) -> Result<Arc<StreamModel>> {
        self.graph.get_stream(stream)
    }

    /// # Failure
    ///
    /// - `SpringError::Sql` when:
    ///   - Name of pump is already used in the same pipeline
    ///   - Name of upstream stream is not found in pipeline
    ///   - Name of downstream stream is not found in pipeline
    pub(super) fn add_pump(&mut self, pump: PumpModel) -> Result<()> {
        self.update_version();
        self.register_name(pump.name().as_ref())?;
        self.graph.add_pump(pump)
    }

    /// # Failure
    ///
    /// - `SpringError::Sql` when:
    ///   - Name of stream is already used in the same pipeline
    pub(super) fn add_stream(&mut self, stream: Arc<StreamModel>) -> Result<()> {
        self.update_version();
        self.register_name(stream.name().as_ref())?;
        self.graph.add_stream(stream)
    }

    /// # Failure
    ///
    /// TODO
    pub(super) fn add_source_reader(&mut self, source_reader: SourceReaderModel) -> Result<()> {
        self.update_version();
        self.graph.add_source_reader(source_reader)
    }
    /// # Failure
    ///
    /// TODO
    pub(super) fn add_sink_writer(&mut self, sink_writer: SinkWriterModel) -> Result<()> {
        self.update_version();
        self.graph.add_sink_writer(sink_writer)
    }

    pub(super) fn all_sources(&self) -> Vec<&SourceReaderModel> {
        self.graph.all_sources()
    }
    pub(super) fn all_sinks(&self) -> Vec<&SinkWriterModel> {
        self.graph.all_sinks()
    }

    /// # Failure
    ///
    /// - `SpringError::Sql` when:
    ///   - Name is already used in the same pipeline
    fn register_name(&mut self, name: &str) -> Result<()> {
        if !self.object_names.insert(name.to_string()) {
            Err(SpringError::Sql(anyhow!(
                r#"name "{}" already exists in pipeline"#,
                name
            )))
        } else {
            Ok(())
        }
    }

    fn update_version(&mut self) {
        self.version.up();
    }
}
