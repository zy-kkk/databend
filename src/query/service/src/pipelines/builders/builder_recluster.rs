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

use common_catalog::plan::DataSourceInfo;
use common_catalog::plan::DataSourcePlan;
use common_exception::ErrorCode;
use common_exception::Result;
use common_expression::DataSchemaRefExt;
use common_expression::SortColumnDescription;
use common_metrics::storage::metrics_inc_recluster_block_bytes_to_read;
use common_metrics::storage::metrics_inc_recluster_block_nums_to_read;
use common_metrics::storage::metrics_inc_recluster_row_nums_to_read;
use common_pipeline_core::processors::ProcessorPtr;
use common_pipeline_sources::EmptySource;
use common_pipeline_transforms::processors::build_merge_sort_pipeline;
use common_pipeline_transforms::processors::AsyncAccumulatingTransformer;
use common_sql::evaluator::CompoundBlockOperator;
use common_sql::executor::physical_plans::MutationKind;
use common_sql::executor::physical_plans::ReclusterSink;
use common_sql::executor::physical_plans::ReclusterSource;
use common_sql::gen_mutation_stream_operator;
use common_storages_factory::Table;
use common_storages_fuse::operations::common::CommitSink;
use common_storages_fuse::operations::common::MutationGenerator;
use common_storages_fuse::operations::ReclusterAggregator;
use common_storages_fuse::operations::TransformSerializeBlock;
use common_storages_fuse::FuseTable;
use common_storages_fuse::TableContext;

use crate::pipelines::processors::TransformAddStreamColumns;
use crate::pipelines::PipelineBuilder;

impl PipelineBuilder {
    pub(crate) fn build_recluster_source(
        &mut self,
        recluster_source: &ReclusterSource,
    ) -> Result<()> {
        match recluster_source.tasks.len() {
            0 => self.main_pipeline.add_source(EmptySource::create, 1),
            1 => {
                let table = self.ctx.build_table_by_table_info(
                    &recluster_source.catalog_info,
                    &recluster_source.table_info,
                    None,
                )?;
                let table = FuseTable::try_from_table(table.as_ref())?;

                let catalog_info = recluster_source.catalog_info.clone();
                let task = &recluster_source.tasks[0];
                let recluster_block_nums = task.parts.len();
                let block_thresholds = table.get_block_thresholds();
                let table_info = table.get_table_info();
                let schema = table.schema_with_stream();
                let description = task.stats.get_description(&table_info.desc);
                let plan = DataSourcePlan {
                    catalog_info,
                    source_info: DataSourceInfo::TableSource(table_info.clone()),
                    output_schema: schema.clone(),
                    parts: task.parts.clone(),
                    statistics: task.stats.clone(),
                    description,
                    tbl_args: table.table_args(),
                    push_downs: None,
                    query_internal_columns: false,
                    base_block_ids: None,
                    update_stream_columns: table.change_tracking_enabled(),
                    data_mask_policy: None,
                };

                self.ctx.set_partitions(plan.parts.clone())?;

                // ReadDataKind to avoid OOM.
                table.do_read_data(self.ctx.clone(), &plan, &mut self.main_pipeline, false)?;

                {
                    metrics_inc_recluster_block_nums_to_read(recluster_block_nums as u64);
                    metrics_inc_recluster_block_bytes_to_read(task.total_bytes as u64);
                    metrics_inc_recluster_row_nums_to_read(task.total_rows as u64);
                }

                let num_input_columns = schema.fields().len();
                if table.change_tracking_enabled() {
                    let func_ctx = self.ctx.get_function_context()?;
                    let (stream, operators) =
                        gen_mutation_stream_operator(schema, table_info.ident.seq)?;
                    self.main_pipeline.add_transform(
                        |transform_input_port, transform_output_port| {
                            TransformAddStreamColumns::try_create(
                                transform_input_port,
                                transform_output_port,
                                CompoundBlockOperator {
                                    operators: operators.clone(),
                                    ctx: func_ctx.clone(),
                                },
                                stream.clone(),
                            )
                        },
                    )?;
                }

                let cluster_stats_gen = table.get_cluster_stats_gen(
                    self.ctx.clone(),
                    task.level + 1,
                    block_thresholds,
                    None,
                )?;
                let operators = cluster_stats_gen.operators.clone();
                if !operators.is_empty() {
                    let func_ctx2 = cluster_stats_gen.func_ctx.clone();
                    self.main_pipeline.add_transform(move |input, output| {
                        Ok(ProcessorPtr::create(CompoundBlockOperator::create(
                            input,
                            output,
                            num_input_columns,
                            func_ctx2.clone(),
                            operators.clone(),
                        )))
                    })?;
                }

                // merge sort
                let block_num = std::cmp::max(
                    task.total_bytes * 80 / (block_thresholds.max_bytes_per_block * 100),
                    1,
                );
                let final_block_size = std::cmp::min(
                    // estimate block_size based on max_bytes_per_block.
                    task.total_rows / block_num,
                    block_thresholds.max_rows_per_block,
                );
                let partial_block_size = if self.main_pipeline.output_len() > 1 {
                    std::cmp::min(
                        final_block_size,
                        self.ctx.get_settings().get_max_block_size()? as usize,
                    )
                } else {
                    final_block_size
                };

                // construct output fields
                let output_fields = cluster_stats_gen.out_fields.clone();
                let schema = DataSchemaRefExt::create(output_fields);
                let sort_descs: Vec<SortColumnDescription> = cluster_stats_gen
                    .cluster_key_index
                    .iter()
                    .map(|offset| SortColumnDescription {
                        offset: *offset,
                        asc: true,
                        nulls_first: false,
                        is_nullable: false, // This information is not needed here.
                    })
                    .collect();

                build_merge_sort_pipeline(
                    &mut self.main_pipeline,
                    schema,
                    sort_descs,
                    None,
                    partial_block_size,
                    final_block_size,
                    None,
                    false,
                    true,
                )?;

                let output_block_num = task.total_rows.div_ceil(final_block_size);
                let max_threads = std::cmp::min(
                    self.ctx.get_settings().get_max_threads()? as usize,
                    output_block_num,
                );
                self.main_pipeline.try_resize(max_threads)?;
                self.main_pipeline
                    .add_transform(|transform_input_port, transform_output_port| {
                        let proc = TransformSerializeBlock::try_create(
                            self.ctx.clone(),
                            transform_input_port,
                            transform_output_port,
                            table,
                            cluster_stats_gen.clone(),
                            MutationKind::Recluster,
                        )?;
                        proc.into_processor()
                    })
            }
            _ => Err(ErrorCode::Internal(
                "A node can only execute one recluster task".to_string(),
            )),
        }
    }

    pub(crate) fn build_recluster_sink(&mut self, recluster_sink: &ReclusterSink) -> Result<()> {
        self.build_pipeline(&recluster_sink.input)?;

        let table = self.ctx.build_table_by_table_info(
            &recluster_sink.catalog_info,
            &recluster_sink.table_info,
            None,
        )?;
        let table = FuseTable::try_from_table(table.as_ref())?;

        self.main_pipeline.try_resize(1)?;
        self.main_pipeline.add_transform(|input, output| {
            let aggregator = ReclusterAggregator::new(
                table,
                self.ctx.clone(),
                recluster_sink.remained_blocks.clone(),
                recluster_sink.removed_segment_indexes.clone(),
                recluster_sink.removed_segment_summary.clone(),
            );
            Ok(ProcessorPtr::create(AsyncAccumulatingTransformer::create(
                input, output, aggregator,
            )))
        })?;

        let snapshot_gen = MutationGenerator::new(recluster_sink.snapshot.clone());
        self.main_pipeline.add_sink(|input| {
            CommitSink::try_create(
                table,
                self.ctx.clone(),
                None,
                vec![],
                snapshot_gen.clone(),
                input,
                None,
                true,
                None,
            )
        })
    }
}
