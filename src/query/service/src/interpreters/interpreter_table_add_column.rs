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

use common_catalog::table::TableExt;
use common_exception::ErrorCode;
use common_exception::Result;
use common_license::license::Feature::ComputedColumn;
use common_license::license_manager::get_license_manager;
use common_meta_app::schema::DatabaseType;
use common_meta_app::schema::UpdateTableMetaReq;
use common_meta_types::MatchSeq;
use common_sql::field_default_value;
use common_sql::plans::AddColumnOption;
use common_sql::plans::AddTableColumnPlan;
use common_storages_share::save_share_table_info;
use common_storages_stream::stream_table::STREAM_ENGINE;
use common_storages_view::view_table::VIEW_ENGINE;

use crate::interpreters::interpreter_table_create::is_valid_column;
use crate::interpreters::Interpreter;
use crate::pipelines::PipelineBuildResult;
use crate::sessions::QueryContext;
use crate::sessions::TableContext;

pub struct AddTableColumnInterpreter {
    ctx: Arc<QueryContext>,
    plan: AddTableColumnPlan,
}

impl AddTableColumnInterpreter {
    pub fn try_create(ctx: Arc<QueryContext>, plan: AddTableColumnPlan) -> Result<Self> {
        Ok(AddTableColumnInterpreter { ctx, plan })
    }
}

#[async_trait::async_trait]
impl Interpreter for AddTableColumnInterpreter {
    fn name(&self) -> &str {
        "AddTableColumnInterpreter"
    }

    #[async_backtrace::framed]
    async fn execute2(&self) -> Result<PipelineBuildResult> {
        let catalog_name = self.plan.catalog.as_str();
        let db_name = self.plan.database.as_str();
        let tbl_name = self.plan.table.as_str();

        let tbl = self
            .ctx
            .get_catalog(catalog_name)
            .await?
            .get_table(self.ctx.get_tenant().as_str(), db_name, tbl_name)
            .await
            .ok();

        if let Some(table) = &tbl {
            // check mutability
            table.check_mutable()?;

            let table_info = table.get_table_info();
            let engine = table_info.engine();
            if matches!(engine, VIEW_ENGINE | STREAM_ENGINE) {
                return Err(ErrorCode::TableEngineNotSupported(format!(
                    "{}.{} engine is {} that doesn't support alter",
                    &self.plan.database, &self.plan.table, engine
                )));
            }
            if table_info.db_type != DatabaseType::NormalDB {
                return Err(ErrorCode::TableEngineNotSupported(format!(
                    "{}.{} doesn't support alter",
                    &self.plan.database, &self.plan.table
                )));
            }

            let catalog = self.ctx.get_catalog(catalog_name).await?;
            let mut new_table_meta = table.get_table_info().meta.clone();
            let field = self.plan.field.clone();
            if field.computed_expr().is_some() {
                let license_manager = get_license_manager();
                license_manager
                    .manager
                    .check_enterprise_enabled(self.ctx.get_license_key(), ComputedColumn)?;
            }

            if field.default_expr().is_some() {
                let _ = field_default_value(self.ctx.clone(), &field)?;
            }
            is_valid_column(field.name())?;
            let index = match &self.plan.option {
                AddColumnOption::First => 0,
                AddColumnOption::After(name) => new_table_meta.schema.index_of(name)? + 1,
                AddColumnOption::End => new_table_meta.schema.num_fields(),
            };
            new_table_meta.add_column(&field, &self.plan.comment, index)?;

            let table_id = table_info.ident.table_id;
            let table_version = table_info.ident.seq;

            let req = UpdateTableMetaReq {
                table_id,
                seq: MatchSeq::Exact(table_version),
                new_table_meta,
                copied_files: None,
                deduplicated_label: None,
                update_stream_meta: vec![],
            };

            let res = catalog.update_table_meta(table_info, req).await?;

            if let Some(share_table_info) = res.share_table_info {
                save_share_table_info(
                    &self.ctx.get_tenant(),
                    self.ctx.get_data_operator()?.operator(),
                    share_table_info,
                )
                .await?;
            }
        };

        Ok(PipelineBuildResult::create())
    }
}
