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

use std::collections::HashMap;

use common_expression::ColumnId;
use common_expression::DataSchemaRef;
use common_expression::FieldIndex;
use common_expression::RemoteExpr;
use common_expression::TableSchemaRef;
use common_meta_app::schema::CatalogInfo;
use common_meta_app::schema::TableInfo;
use storages_common_table_meta::meta::ColumnStatistics;

use crate::executor::physical_plans::common::OnConflictField;
use crate::executor::PhysicalPlan;
use crate::ColumnBinding;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ReplaceDeduplicate {
    pub input: Box<PhysicalPlan>,
    pub on_conflicts: Vec<OnConflictField>,
    pub bloom_filter_column_indexes: Vec<FieldIndex>,
    pub table_is_empty: bool,
    pub table_info: TableInfo,
    pub catalog_info: CatalogInfo,
    pub table_schema: TableSchemaRef,
    pub select_ctx: Option<ReplaceSelectCtx>,
    pub table_level_range_index: HashMap<ColumnId, ColumnStatistics>,
    pub need_insert: bool,
    pub delete_when: Option<(RemoteExpr, String)>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ReplaceSelectCtx {
    pub select_column_bindings: Vec<ColumnBinding>,
    pub select_schema: DataSchemaRef,
}
