use crate::app::App;
use crate::out;
use crate::Cli;
use anyhow::Result;
use clap::Subcommand;
use grammers_tl_types as tl;
use serde::Serialize;

#[derive(Subcommand, Debug, Clone)]
pub enum StickersCommand {
    /// List user's saved/favorite sticker packs
    List,
    /// Show stickers in a pack
    Show {
        /// Sticker pack ID (or short name)
        #[arg(long)]
        pack: String,
    },
    /// Search stickers by emoji
    Search {
        /// Emoji to search for
        #[arg(long)]
        emoji: String,
        /// Limit results
        #[arg(long, default_value = "20")]
        limit: usize,
    },
}

#[derive(Serialize)]
struct StickerPackInfo {
    id: i64,
    access_hash: i64,
    short_name: String,
    title: String,
    count: i32,
    official: bool,
    emojis: bool,
}

#[derive(Serialize)]
struct StickerInfo {
    emoji: String,
    file_id: String, // Encoded as doc_id:access_hash:file_ref_base64
    doc_id: i64,
    animated: bool,
}

/// Encode a sticker's document info into a portable file_id string.
/// Format: {doc_id}:{access_hash}:{file_ref_base64}
fn encode_file_id(doc_id: i64, access_hash: i64, file_reference: &[u8]) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    let file_ref_b64 = URL_SAFE_NO_PAD.encode(file_reference);
    format!("{}:{}:{}", doc_id, access_hash, file_ref_b64)
}

/// Decode a file_id string back to its components.
/// Returns (doc_id, access_hash, file_reference)
#[allow(dead_code)]
pub fn decode_file_id(file_id: &str) -> Result<(i64, i64, Vec<u8>)> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    let parts: Vec<&str> = file_id.split(':').collect();
    if parts.len() != 3 {
        anyhow::bail!("Invalid sticker file_id format. Use `tgcli stickers show --pack <pack_name>` to get valid file IDs.");
    }
    let doc_id: i64 = parts[0].parse()?;
    let access_hash: i64 = parts[1].parse()?;
    let file_reference = URL_SAFE_NO_PAD.decode(parts[2])?;
    Ok((doc_id, access_hash, file_reference))
}

pub async fn run(cli: &Cli, cmd: &StickersCommand) -> Result<()> {
    let app = App::new(cli).await?;

    match cmd {
        StickersCommand::List => list_sticker_packs(&app, cli).await,
        StickersCommand::Show { pack } => show_sticker_pack(&app, cli, pack).await,
        StickersCommand::Search { emoji, limit } => search_stickers(&app, cli, emoji, *limit).await,
    }
}

async fn list_sticker_packs(app: &App, cli: &Cli) -> Result<()> {
    // Get all installed sticker sets
    let request = tl::functions::messages::GetAllStickers { hash: 0 };
    let result = app.tg.client.invoke(&request).await?;

    let sets = match result {
        tl::enums::messages::AllStickers::Stickers(stickers) => stickers.sets,
        tl::enums::messages::AllStickers::NotModified => {
            anyhow::bail!("Sticker sets not modified (unexpected)");
        }
    };

    let packs: Vec<StickerPackInfo> = sets
        .into_iter()
        .map(|set| {
            let tl::enums::StickerSet::Set(s) = set;
            StickerPackInfo {
                id: s.id,
                access_hash: s.access_hash,
                short_name: s.short_name,
                title: s.title,
                count: s.count,
                official: s.official,
                emojis: s.emojis,
            }
        })
        .collect();

    if cli.json {
        out::write_json(&serde_json::json!({
            "count": packs.len(),
            "packs": packs,
        }))?;
    } else {
        println!(
            "{:<20} {:<40} {:<8} {:<8} {:<6}",
            "SHORT_NAME", "TITLE", "COUNT", "OFFICIAL", "EMOJI"
        );
        for p in &packs {
            let title = out::truncate(&p.title, 38);
            let official = if p.official { "yes" } else { "no" };
            let emoji = if p.emojis { "yes" } else { "no" };
            println!(
                "{:<20} {:<40} {:<8} {:<8} {:<6}",
                out::truncate(&p.short_name, 18),
                title,
                p.count,
                official,
                emoji
            );
        }
        println!("\n{} sticker pack(s) installed", packs.len());
    }
    Ok(())
}

async fn show_sticker_pack(app: &App, cli: &Cli, pack: &str) -> Result<()> {
    // Use short name to look up the sticker set
    // (Numeric IDs require access_hash which we don't have without listing first)
    let input_sticker_set =
        tl::enums::InputStickerSet::ShortName(tl::types::InputStickerSetShortName {
            short_name: pack.to_string(),
        });

    let request = tl::functions::messages::GetStickerSet {
        stickerset: input_sticker_set,
        hash: 0,
    };

    let result = app.tg.client.invoke(&request).await?;

    let (set_info, documents) = match result {
        tl::enums::messages::StickerSet::Set(s) => (s.set, s.documents),
        tl::enums::messages::StickerSet::NotModified => {
            anyhow::bail!("Sticker set not modified (unexpected)");
        }
    };

    let tl::enums::StickerSet::Set(set_data) = &set_info;
    let set_title = set_data.title.clone();

    let stickers: Vec<StickerInfo> = documents
        .into_iter()
        .filter_map(|doc| {
            if let tl::enums::Document::Document(d) = doc {
                // Find the sticker attribute to get emoji
                let emoji = d
                    .attributes
                    .iter()
                    .find_map(|attr| {
                        if let tl::enums::DocumentAttribute::Sticker(s) = attr {
                            Some(s.alt.clone())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_default();

                let animated = d
                    .attributes
                    .iter()
                    .any(|attr| matches!(attr, tl::enums::DocumentAttribute::Animated));

                Some(StickerInfo {
                    emoji,
                    file_id: encode_file_id(d.id, d.access_hash, &d.file_reference),
                    doc_id: d.id,
                    animated,
                })
            } else {
                None
            }
        })
        .collect();

    if cli.json {
        out::write_json(&serde_json::json!({
            "pack": pack,
            "title": set_title,
            "count": stickers.len(),
            "stickers": stickers,
        }))?;
    } else {
        println!("Pack: {} ({})\n", set_title, pack);
        println!("{:<6} {:<20} FILE_ID", "EMOJI", "DOC_ID");
        for s in &stickers {
            println!(
                "{:<6} {:<20} {}",
                s.emoji,
                s.doc_id,
                &s.file_id[..s.file_id.len().min(50)]
            );
        }
        println!("\n{} sticker(s) in pack", stickers.len());
        println!("\nUse the FILE_ID with: tgcli send --to <chat_id> --sticker <file_id>");
    }
    Ok(())
}

async fn search_stickers(app: &App, cli: &Cli, emoji: &str, limit: usize) -> Result<()> {
    // Search for stickers by emoji using getStickers
    let request = tl::functions::messages::GetStickers {
        emoticon: emoji.to_string(),
        hash: 0,
    };

    let result = app.tg.client.invoke(&request).await?;

    let documents = match result {
        tl::enums::messages::Stickers::Stickers(s) => s.stickers,
        tl::enums::messages::Stickers::NotModified => {
            anyhow::bail!("Stickers not modified (unexpected)");
        }
    };

    let stickers: Vec<StickerInfo> = documents
        .into_iter()
        .take(limit)
        .filter_map(|doc| {
            if let tl::enums::Document::Document(d) = doc {
                let sticker_emoji = d
                    .attributes
                    .iter()
                    .find_map(|attr| {
                        if let tl::enums::DocumentAttribute::Sticker(s) = attr {
                            Some(s.alt.clone())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| emoji.to_string());

                let animated = d
                    .attributes
                    .iter()
                    .any(|attr| matches!(attr, tl::enums::DocumentAttribute::Animated));

                Some(StickerInfo {
                    emoji: sticker_emoji,
                    file_id: encode_file_id(d.id, d.access_hash, &d.file_reference),
                    doc_id: d.id,
                    animated,
                })
            } else {
                None
            }
        })
        .collect();

    if cli.json {
        out::write_json(&serde_json::json!({
            "emoji": emoji,
            "count": stickers.len(),
            "stickers": stickers,
        }))?;
    } else {
        println!("Stickers for emoji: {}\n", emoji);
        println!("{:<6} {:<20} FILE_ID", "EMOJI", "DOC_ID");
        for s in &stickers {
            println!(
                "{:<6} {:<20} {}",
                s.emoji,
                s.doc_id,
                &s.file_id[..s.file_id.len().min(50)]
            );
        }
        println!("\n{} sticker(s) found", stickers.len());
    }
    Ok(())
}
