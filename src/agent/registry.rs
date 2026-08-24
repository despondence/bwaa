// src/agent/registry.rs
use std::collections::HashMap;
use std::sync::Arc;

use crate::agent::tool::Tool;
use crate::gemini::convert::json_to_gemini_schema;
use crate::googleapis::google::ai::generativelanguage::v1beta::{
    FunctionDeclaration, Tool as GeminiTool,
};

#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: Arc<HashMap<String, Arc<dyn Tool>>>,
}

impl ToolRegistry {
    pub fn builder() -> ToolRegistryBuilder {
        ToolRegistryBuilder::default()
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    /// Converts all registered tools into Gemini's gRPC Tool protobuf structure
    pub fn to_gemini_tool(&self) -> GeminiTool {
        let declarations = self
            .tools
            .values()
            .map(|t| {
                let schema = json_to_gemini_schema(&t.parameters_schema());

                FunctionDeclaration {
                    name: t.name().to_string(),
                    description: t.description().to_string(),
                    parameters: Some(schema),
                    ..Default::default()
                }
            })
            .collect();

        GeminiTool {
            function_declarations: declarations,
            ..Default::default()
        }
    }
}

#[derive(Default)]
pub struct ToolRegistryBuilder {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistryBuilder {
    pub fn register<T: Tool + 'static>(mut self, tool: T) -> Self {
        self.tools.insert(tool.name().to_string(), Arc::new(tool));
        self
    }

    pub fn build(self) -> ToolRegistry {
        ToolRegistry {
            tools: Arc::new(self.tools),
        }
    }
}
