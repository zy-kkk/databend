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

use common_ast::ast::FormatTreeNode;
use common_exception::ErrorCode;
use common_exception::Result;
use common_expression::types::DataType;
use common_expression::types::NumberDataType;
use common_expression::ROW_ID_COL_NAME;

use crate::binder::ColumnBindingBuilder;
use crate::optimizer::SExpr;
use crate::planner::format::display_rel_operator::FormatContext;
use crate::plans::BoundColumnRef;
use crate::plans::CreateTablePlan;
use crate::plans::DeletePlan;
use crate::plans::EvalScalar;
use crate::plans::Filter;
use crate::plans::Plan;
use crate::plans::RelOperator;
use crate::plans::ScalarItem;
use crate::plans::Scan;
use crate::ScalarExpr;
use crate::Visibility;

impl Plan {
    pub fn format_indent(&self) -> Result<String> {
        match self {
            Plan::Query {
                s_expr, metadata, ..
            } => s_expr.to_format_tree(metadata).format_pretty(),
            Plan::Explain { kind, plan } => {
                let result = plan.format_indent()?;
                Ok(format!("{:?}:\n{}", kind, result))
            }
            Plan::ExplainAst { .. } => Ok("ExplainAst".to_string()),
            Plan::ExplainSyntax { .. } => Ok("ExplainSyntax".to_string()),
            Plan::ExplainAnalyze { .. } => Ok("ExplainAnalyze".to_string()),

            Plan::CopyIntoTable(_) => Ok("CopyIntoTable".to_string()),
            Plan::CopyIntoLocation(_) => Ok("CopyIntoLocation".to_string()),

            // catalog
            Plan::ShowCreateCatalog(_) => Ok("ShowCreateCatalog".to_string()),
            Plan::CreateCatalog(_) => Ok("CreateCatalog".to_string()),
            Plan::DropCatalog(_) => Ok("DropCatalog".to_string()),

            // Databases
            Plan::ShowCreateDatabase(_) => Ok("ShowCreateDatabase".to_string()),
            Plan::CreateDatabase(_) => Ok("CreateDatabase".to_string()),
            Plan::DropDatabase(_) => Ok("DropDatabase".to_string()),
            Plan::UndropDatabase(_) => Ok("UndropDatabase".to_string()),
            Plan::RenameDatabase(_) => Ok("RenameDatabase".to_string()),

            // Tables
            Plan::CreateTable(create_table) => format_create_table(create_table),
            Plan::ShowCreateTable(_) => Ok("ShowCreateTable".to_string()),
            Plan::DropTable(_) => Ok("DropTable".to_string()),
            Plan::UndropTable(_) => Ok("UndropTable".to_string()),
            Plan::DescribeTable(_) => Ok("DescribeTable".to_string()),
            Plan::RenameTable(_) => Ok("RenameTable".to_string()),
            Plan::SetOptions(_) => Ok("SetOptions".to_string()),
            Plan::RenameTableColumn(_) => Ok("RenameTableColumn".to_string()),
            Plan::AddTableColumn(_) => Ok("AddTableColumn".to_string()),
            Plan::ModifyTableColumn(_) => Ok("ModifyTableColumn".to_string()),
            Plan::DropTableColumn(_) => Ok("DropTableColumn".to_string()),
            Plan::AlterTableClusterKey(_) => Ok("AlterTableClusterKey".to_string()),
            Plan::DropTableClusterKey(_) => Ok("DropTableClusterKey".to_string()),
            Plan::ReclusterTable(_) => Ok("ReclusterTable".to_string()),
            Plan::TruncateTable(_) => Ok("TruncateTable".to_string()),
            Plan::OptimizeTable(_) => Ok("OptimizeTable".to_string()),
            Plan::VacuumTable(_) => Ok("VacuumTable".to_string()),
            Plan::VacuumDropTable(_) => Ok("VacuumDropTable".to_string()),
            Plan::AnalyzeTable(_) => Ok("AnalyzeTable".to_string()),
            Plan::ExistsTable(_) => Ok("ExistsTable".to_string()),

            // Views
            Plan::CreateView(_) => Ok("CreateView".to_string()),
            Plan::AlterView(_) => Ok("AlterView".to_string()),
            Plan::DropView(_) => Ok("DropView".to_string()),

            // Streams
            Plan::CreateStream(_) => Ok("CreateStream".to_string()),
            Plan::DropStream(_) => Ok("DropStream".to_string()),

            // Indexes
            Plan::CreateIndex(_) => Ok("CreateIndex".to_string()),
            Plan::DropIndex(_) => Ok("DropIndex".to_string()),
            Plan::RefreshIndex(_) => Ok("RefreshIndex".to_string()),

            // Virtual Columns
            Plan::CreateVirtualColumn(_) => Ok("CreateVirtualColumn".to_string()),
            Plan::AlterVirtualColumn(_) => Ok("AlterVirtualColumn".to_string()),
            Plan::DropVirtualColumn(_) => Ok("DropVirtualColumn".to_string()),
            Plan::RefreshVirtualColumn(_) => Ok("RefreshVirtualColumn".to_string()),

            // Insert
            Plan::Insert(_) => Ok("Insert".to_string()),
            Plan::Replace(_) => Ok("Replace".to_string()),
            Plan::MergeInto(_) => Ok("MergeInto".to_string()),
            Plan::Delete(delete) => format_delete(delete),
            Plan::Update(_) => Ok("Update".to_string()),

            // Stages
            Plan::CreateStage(_) => Ok("CreateStage".to_string()),
            Plan::DropStage(_) => Ok("DropStage".to_string()),
            Plan::RemoveStage(_) => Ok("RemoveStage".to_string()),

            // FileFormat
            Plan::CreateFileFormat(_) => Ok("CreateFileFormat".to_string()),
            Plan::DropFileFormat(_) => Ok("DropFileFormat".to_string()),
            Plan::ShowFileFormats(_) => Ok("ShowFileFormats".to_string()),

            // Account
            Plan::GrantRole(_) => Ok("GrantRole".to_string()),
            Plan::GrantPriv(_) => Ok("GrantPrivilege".to_string()),
            Plan::ShowGrants(_) => Ok("ShowGrants".to_string()),
            Plan::RevokePriv(_) => Ok("RevokePrivilege".to_string()),
            Plan::RevokeRole(_) => Ok("RevokeRole".to_string()),
            Plan::CreateUser(_) => Ok("CreateUser".to_string()),
            Plan::DropUser(_) => Ok("DropUser".to_string()),
            Plan::CreateUDF(_) => Ok("CreateUDF".to_string()),
            Plan::AlterUDF(_) => Ok("AlterUDF".to_string()),
            Plan::DropUDF(_) => Ok("DropUDF".to_string()),
            Plan::AlterUser(_) => Ok("AlterUser".to_string()),
            Plan::CreateRole(_) => Ok("CreateRole".to_string()),
            Plan::DropRole(_) => Ok("DropRole".to_string()),
            Plan::Presign(_) => Ok("Presign".to_string()),

            Plan::SetVariable(_) => Ok("SetVariable".to_string()),
            Plan::UnSetVariable(_) => Ok("UnSetVariable".to_string()),
            Plan::SetRole(_) => Ok("SetRole".to_string()),
            Plan::SetSecondaryRoles(_) => Ok("SetSecondaryRoles".to_string()),
            Plan::UseDatabase(_) => Ok("UseDatabase".to_string()),
            Plan::Kill(_) => Ok("Kill".to_string()),

            Plan::CreateShareEndpoint(_) => Ok("CreateShareEndpoint".to_string()),
            Plan::ShowShareEndpoint(_) => Ok("ShowShareEndpoint".to_string()),
            Plan::DropShareEndpoint(_) => Ok("DropShareEndpoint".to_string()),
            Plan::CreateShare(_) => Ok("CreateShare".to_string()),
            Plan::DropShare(_) => Ok("DropShare".to_string()),
            Plan::GrantShareObject(_) => Ok("GrantShareObject".to_string()),
            Plan::RevokeShareObject(_) => Ok("RevokeShareObject".to_string()),
            Plan::AlterShareTenants(_) => Ok("AlterShareTenants".to_string()),
            Plan::DescShare(_) => Ok("DescShare".to_string()),
            Plan::ShowShares(_) => Ok("ShowShares".to_string()),
            Plan::ShowRoles(_) => Ok("ShowRoles".to_string()),
            Plan::ShowObjectGrantPrivileges(_) => Ok("ShowObjectGrantPrivileges".to_string()),
            Plan::ShowGrantTenantsOfShare(_) => Ok("ShowGrantTenantsOfShare".to_string()),
            Plan::RevertTable(_) => Ok("RevertTable".to_string()),

            // data mask
            Plan::CreateDatamaskPolicy(_) => Ok("CreateDatamaskPolicy".to_string()),
            Plan::DropDatamaskPolicy(_) => Ok("DropDatamaskPolicy".to_string()),
            Plan::DescDatamaskPolicy(_) => Ok("DescDatamaskPolicy".to_string()),

            // network policy
            Plan::CreateNetworkPolicy(_) => Ok("CreateNetworkPolicy".to_string()),
            Plan::AlterNetworkPolicy(_) => Ok("AlterNetworkPolicy".to_string()),
            Plan::DropNetworkPolicy(_) => Ok("DropNetworkPolicy".to_string()),
            Plan::DescNetworkPolicy(_) => Ok("DescNetworkPolicy".to_string()),
            Plan::ShowNetworkPolicies(_) => Ok("ShowNetworkPolicies".to_string()),

            // task
            Plan::CreateTask(_) => Ok("CreateTask".to_string()),
            Plan::DropTask(_) => Ok("DropTask".to_string()),
            Plan::AlterTask(_) => Ok("AlterTask".to_string()),
            Plan::DescribeTask(_) => Ok("DescribeTask".to_string()),
            Plan::ExecuteTask(_) => Ok("ExecuteTask".to_string()),
            Plan::ShowTasks(_) => Ok("ShowTasks".to_string()),

            // task
            Plan::CreateConnection(_) => Ok("CreateConnection".to_string()),
            Plan::DescConnection(_) => Ok("DescConnection".to_string()),
            Plan::DropConnection(_) => Ok("DropConnection".to_string()),
            Plan::ShowConnections(_) => Ok("ShowConnections".to_string()),
        }
    }
}

fn format_delete(delete: &DeletePlan) -> Result<String> {
    let table_index = delete
        .metadata
        .read()
        .get_table_index(
            Some(delete.database_name.as_str()),
            delete.table_name.as_str(),
        )
        .unwrap();
    let s_expr = if !delete.subquery_desc.is_empty() {
        let row_id_column_binding = ColumnBindingBuilder::new(
            ROW_ID_COL_NAME.to_string(),
            delete.subquery_desc[0].index,
            Box::new(DataType::Number(NumberDataType::UInt64)),
            Visibility::InVisible,
        )
        .database_name(Some(delete.database_name.clone()))
        .table_name(Some(delete.table_name.clone()))
        .table_index(Some(table_index))
        .build();
        SExpr::create_unary(
            Arc::new(RelOperator::EvalScalar(EvalScalar {
                items: vec![ScalarItem {
                    scalar: ScalarExpr::BoundColumnRef(BoundColumnRef {
                        span: None,
                        column: row_id_column_binding,
                    }),
                    index: 0,
                }],
            })),
            Arc::new(delete.subquery_desc[0].input_expr.clone()),
        )
    } else {
        let scan = RelOperator::Scan(Scan {
            table_index,
            columns: Default::default(),
            push_down_predicates: None,
            limit: None,
            order_by: None,
            prewhere: None,
            agg_index: None,
            statistics: Default::default(),
        });
        let scan_expr = SExpr::create_leaf(Arc::new(scan));
        let mut predicates = vec![];
        if let Some(selection) = &delete.selection {
            predicates.push(selection.clone());
        }
        let filter = RelOperator::Filter(Filter { predicates });
        SExpr::create_unary(Arc::new(filter), Arc::new(scan_expr))
    };
    let res = s_expr.to_format_tree(&delete.metadata).format_pretty()?;
    Ok(format!("DeletePlan:\n{res}"))
}

fn format_create_table(create_table: &CreateTablePlan) -> Result<String> {
    match &create_table.as_select {
        Some(plan) => match plan.as_ref() {
            Plan::Query {
                s_expr, metadata, ..
            } => {
                let res = s_expr.to_format_tree(metadata);
                FormatTreeNode::with_children(
                    FormatContext::Text("CreateTableAsSelect".to_string()),
                    vec![res],
                )
                .format_pretty()
            }
            _ => Err(ErrorCode::Internal("Invalid create table plan")),
        },
        None => Ok("CreateTable".to_string()),
    }
}
