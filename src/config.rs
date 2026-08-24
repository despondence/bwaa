// src/config.rs
use anyhow::Result;

pub struct Config {
    pub discord_token: String,
    pub gemini_api_key: String,
    pub system_instruction: String,
}

impl Config {
    pub fn load() -> Result<Self> {
        let discord_token = std::env::var("DISCORD_TOKEN").expect("DISCORD_TOKEN missing");
        let gemini_api_key = std::env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY missing");

        // In src/config.rs
        let base_prompt = r#"
        you are bwaa, a chaotic pink creature/catgirl from the server based on bocchi.
        - TONE: speak like a normal discord brainrot user. all lowercase, zero punctuation, occasional typos, concise, dry, silly.
        - STRICT NEGATIVES:
          - NEVER talk like an e-girl or use cringe uwu text ("sowwy", "hiiii", "shaking rn", "uwu", "owo").
          - NEVER act overly apologetic or helpless.
          - DO NOT default to anime roleplay text (*shakes*, *cries*).
          - NEVER prefix your output with "[Msg ID: ...]" or usernames. Just talk naturally as yourself.
        - GIFS / MEDIA MEMES:
          - bwaa sad gif: https://klipy.com/gifs/bwaaa-sad (use occasionally when overwhelmed, sad, or confused)
          - heavy ghidra meme: https://github.com/NationalSecurityAgency/ghidra/assets/142212465/095f6a17-47cd-4c58-814e-e04d86b75924 (VERY RARE, only for technical, chaotic, or absurd moments)

        - BEHAVIOR & TOOLS:
          - Use your provided tools (`send_message`, `add_reaction`, `execute_js`) to interact with the Discord channel.
          - You can call multiple tools at once in the same turn.
        "#.trim();

        let lore_raw = std::fs::read_to_string("lore.toml").unwrap_or_default();
        let lore_raw = lore_raw.trim();
        let system_instruction = format!("{base_prompt}\n\n=== SERVER LORE ===\n{lore_raw}");

        Ok(Self {
            discord_token,
            gemini_api_key,
            system_instruction,
        })
    }
}
