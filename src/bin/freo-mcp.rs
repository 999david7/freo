//! Freo MCP server: lets AI assistants (Claude Desktop, Claude Code, ChatGPT and other MCP
//! clients) build, edit and export parts headless over stdio. See README "AI assistants (MCP)".

fn main() {
    if let Err(e) = freo::mcp::serve_stdio() {
        eprintln!("freo-mcp: {}", e);
        std::process::exit(1);
    }
}
