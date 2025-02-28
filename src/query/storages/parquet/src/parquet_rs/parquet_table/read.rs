// Copyright 2021 Datafuse Labs
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::sync::Arc;

use common_catalog::plan::DataSourcePlan;
use common_catalog::table::Table;
use common_catalog::table_context::TableContext;
use common_exception::Result;
use common_expression::TableSchemaRef;
use common_pipeline_core::Pipeline;
use storages_common_index::Index;
use storages_common_index::RangeIndex;

use super::ParquetRSTable;
use crate::parquet_rs::source::ParquetSource;
use crate::utils::calc_parallelism;
use crate::ParquetPart;
use crate::ParquetRSPruner;
use crate::ParquetRSReaderBuilder;

impl ParquetRSTable {
    #[inline]
    pub(super) fn do_read_data(
        &self,
        ctx: Arc<dyn TableContext>,
        plan: &DataSourcePlan,
        pipeline: &mut Pipeline,
    ) -> Result<()> {
        let table_schema: TableSchemaRef = self.table_info.schema();
        // If there is a `ParquetFilesPart`, we should create pruner for it.
        // `ParquetFilesPart`s are always staying at the end of `parts`.
        let has_files_part = matches!(
            plan.parts
                .partitions
                .last()
                .map(|p| p.as_any().downcast_ref::<ParquetPart>().unwrap()),
            Some(ParquetPart::ParquetFiles(_)),
        );
        let pruner = if has_files_part {
            Some(ParquetRSPruner::try_create(
                ctx.get_function_context()?,
                table_schema.clone(),
                self.leaf_fields.clone(),
                &plan.push_downs,
                self.read_options,
            )?)
        } else {
            None
        };

        let num_threads = calc_parallelism(&ctx, plan)?;

        let topk = plan
            .push_downs
            .as_ref()
            .and_then(|p| p.top_k(&self.schema(), RangeIndex::supported_type));

        let mut builder = ParquetRSReaderBuilder::create_with_parquet_schema(
            ctx.clone(),
            self.operator.clone(),
            table_schema,
            self.schema_descr.clone(),
        )
        .with_options(self.read_options)
        .with_push_downs(plan.push_downs.as_ref())
        .with_pruner(pruner)
        .with_topk(topk.as_ref());

        let row_group_reader = Arc::new(builder.build_row_group_reader()?);
        let full_file_reader = if has_files_part {
            Some(Arc::new(builder.build_full_reader()?))
        } else {
            None
        };

        let topk = Arc::new(topk);
        pipeline.add_source(
            |output| {
                ParquetSource::create(
                    ctx.clone(),
                    output,
                    row_group_reader.clone(),
                    full_file_reader.clone(),
                    topk.clone(),
                )
            },
            num_threads,
        )
    }
}
