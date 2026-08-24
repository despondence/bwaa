// src/gemini/builder.rs
use crate::googleapis::google::ai::generativelanguage::v1beta::part::Data;
use crate::googleapis::google::ai::generativelanguage::v1beta::{
    Content, GenerateContentRequest, Part, Tool,
};

pub struct RequestBuilder {
    model: String,
    system_instruction: Option<String>,
    history: Vec<Content>,
    tools: Vec<Tool>,
}

impl RequestBuilder {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            system_instruction: None,
            history: Vec::new(),
            tools: Vec::new(),
        }
    }

    pub fn system_instruction(mut self, prompt: impl Into<String>) -> Self {
        self.system_instruction = Some(prompt.into());
        self
    }

    pub fn history(mut self, contents: Vec<Content>) -> Self {
        self.history = contents;
        self
    }

    pub fn tool(mut self, tool: Tool) -> Self {
        self.tools.push(tool);
        self
    }

    pub fn build(self) -> GenerateContentRequest {
        GenerateContentRequest {
            model: self.model,
            contents: self.history,
            system_instruction: self.system_instruction.map(|prompt| Content {
                role: "system".into(),
                parts: vec![Part {
                    data: Some(Data::Text(prompt)),
                    ..Default::default()
                }],
            }),
            tools: self.tools,
            ..Default::default()
        }
    }
}
