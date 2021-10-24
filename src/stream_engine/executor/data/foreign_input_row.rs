pub(in crate::stream_engine) mod format;

use self::format::json::JsonObject;

use crate::{error::Result, model::stream_model::StreamModel};

use super::row::Row;

/// Input row from foreign systems (retrieved from InputServer).
///
/// Immediately converted into Row on stream-engine boundary.
#[derive(Eq, PartialEq, Debug)]
pub(in crate::stream_engine::executor) struct ForeignInputRow(JsonObject);

impl ForeignInputRow {
    pub(in crate::stream_engine::executor) fn from_json(json: JsonObject) -> Self {
        Self(json)
    }

    pub(in crate::stream_engine::executor) fn _into_row(self, _stream: &StreamModel) -> Result<Row> {
        todo!()
    }
}
