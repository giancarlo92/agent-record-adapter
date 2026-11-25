// Part of Agent Record Adapter (see NOTICE).
//! `skills`: agent-desktop's own skills (from the engine) plus ours, built in,
//! so an agent without a skills folder can read them through the CLI or MCP.

use std::process::ExitCode;

use serde_json::{Value, json};

struct Skill {
    name: &'static str,
    aliases: &'static [&'static str],
    summary: &'static str,
    content: &'static str,
    references: &'static [(&'static str, &'static str)],
}

const SKILLS: &[Skill] = &[
    Skill {
        name: "agent-record-background",
        aliases: &["background", "record-background"],
        summary: "Agent Record Adapter's layer over agent-desktop and agent-browser: background-first routes, the added commands (menu, click-at, navigate, type w-<id>, stroke, secret, record), web pages through `browser`.",
        content: include_str!("../../../skills/agent-record-background/SKILL.md"),
        references: &[],
    },
    Skill {
        name: "desktop-recording",
        aliases: &["recording"],
        summary: "Record what a person (or an agent) does in desktop apps and web pages, in the background, and read the recording.",
        content: include_str!("../../../skills/desktop-recording/SKILL.md"),
        references: &[],
    },
    Skill {
        name: "recording-to-script",
        aliases: &["script"],
        summary: "Turn a recording or a task done with the tools into a script that repeats it without AI, in the background.",
        content: include_str!("../../../skills/recording-to-script/SKILL.md"),
        references: &[(
            "desktop-script.mjs",
            include_str!("../../../skills/recording-to-script/scripts/desktop-script.mjs"),
        )],
    },
    Skill {
        name: "script-repair",
        aliases: &["repair"],
        summary: "Repair a script made from a recording when the app or page changed: fix only the first step that no longer matches.",
        content: include_str!("../../../skills/script-repair/SKILL.md"),
        references: &[],
    },
];

/// The desktop-script.mjs this agent-record installs with its skills.
pub(crate) fn helper_source() -> &'static str {
    include_str!("../../../skills/recording-to-script/scripts/desktop-script.mjs")
}

fn find(name: &str) -> Option<&'static Skill> {
    SKILLS
        .iter()
        .find(|skill| skill.name == name || skill.aliases.contains(&name))
}

/// Handles `skills list` and `skills get <ours>`; `None` leaves the command
/// to the engine (its own skills, `skills path`).
pub(crate) fn run(args: &[String]) -> Option<ExitCode> {
    let words: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .skip_while(|arg| *arg != "skills")
        .skip(1)
        .filter(|arg| !arg.starts_with('-'))
        .collect();
    match words.as_slice() {
        [] | ["list"] => Some(list(args)),
        ["get", name, rest @ ..] => {
            let skill = find(name)?;
            let full = args.iter().any(|arg| arg == "--full");
            Some(crate::run::emit(
                "skills",
                get(skill, rest.first().copied(), full),
            ))
        }
        _ => None,
    }
}

fn list(args: &[String]) -> ExitCode {
    let mut skills = engine_skills(args).unwrap_or_default();
    for skill in SKILLS {
        skills.push(json!({
            "name": skill.name,
            "aliases": skill.aliases,
            "summary": skill.summary,
            "references": skill.references.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            "source": "desktop-recorder-mcp",
        }));
    }
    crate::run::emit("skills", Ok(json!({ "skills": skills })))
}

fn engine_skills(args: &[String]) -> Option<Vec<Value>> {
    let output = crate::engine::command(args).ok()?.output().ok()?;
    let envelope: Value = serde_json::from_slice(&output.stdout).ok()?;
    envelope["data"]["skills"].as_array().cloned()
}

fn get(
    skill: &Skill,
    reference: Option<&str>,
    full: bool,
) -> Result<Value, agent_desktop_core::AppError> {
    let content = match reference {
        None if full => {
            let mut text = skill.content.to_string();
            for (name, body) in skill.references {
                text.push_str(&format!("\n\n---\n# {name}\n\n{body}"));
            }
            text
        }
        None => skill.content.to_string(),
        Some(wanted) => skill
            .references
            .iter()
            .find(|(name, _)| wanted.ends_with(name) || name.starts_with(wanted))
            .map(|(_, body)| (*body).to_string())
            .ok_or_else(|| {
                agent_desktop_core::AppError::invalid_input(format!(
                    "Skill '{}' has no reference '{wanted}'",
                    skill.name
                ))
            })?,
    };
    Ok(json!({ "name": skill.name, "content": content }))
}
