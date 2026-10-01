use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serenity::builder::{
    CreateActionRow, CreateButton, CreateEmbed, CreateEmbedFooter, CreateInteractionResponse,
    CreateInteractionResponseMessage, CreateMessage,
};
use serenity::builder::EditMember;
use serenity::http::Http;
use serenity::model::application::{ButtonStyle, ComponentInteraction};
use serenity::model::id::{ChannelId, GuildId, MessageId, UserId};
use serenity::model::Permissions;
use serenity::model::Timestamp;
use serenity::prelude::*;

use crate::ai_moderator::ChatEntry;

// Color palettes for mod alerts
const COLOR_RED: u32 = 0xED4245;
const COLOR_ORANGE: u32 = 0xFEE75C;
const COLOR_GREEN: u32 = 0x57F287;

/// Generates interactive button rows for automod alerts
pub fn create_mod_action_rows(id_prefix: &str, is_auto_deleted: bool) -> Vec<CreateActionRow> {
    // Row 1: Timeouts & False Positive (10m, 1h, 4h, 24h, False Positive)
    let btn_m10m = CreateButton::new(format!("mod_m10:{}", id_prefix))
        .label("10m")
        .style(ButtonStyle::Secondary)
        .emoji('⏱');

    let btn_m1h = CreateButton::new(format!("mod_m1h:{}", id_prefix))
        .label("1h")
        .style(ButtonStyle::Primary)
        .emoji('⏳');

    let btn_m4h = CreateButton::new(format!("mod_m4h:{}", id_prefix))
        .label("4h")
        .style(ButtonStyle::Primary)
        .emoji('⏰');

    let btn_m1d = CreateButton::new(format!("mod_m1d:{}", id_prefix))
        .label("24h")
        .style(ButtonStyle::Primary)
        .emoji('🛑');

    let btn_fp = CreateButton::new(format!("mod_fp:{}", id_prefix))
        .label("False Positive")
        .style(ButtonStyle::Success)
        .emoji('✅');

    let row1 = CreateActionRow::Buttons(vec![btn_m10m, btn_m1h, btn_m4h, btn_m1d, btn_fp]);

    // Row 2: Escalations & Utilities
    let btn_kick = CreateButton::new(format!("mod_kick:{}", id_prefix))
        .label("Kick")
        .style(ButtonStyle::Danger)
        .emoji('👢');

    let btn_softban = CreateButton::new(format!("mod_softban:{}", id_prefix))
        .label("Softban")
        .style(ButtonStyle::Danger)
        .emoji('🧹');

    let btn_ban = CreateButton::new(format!("mod_ban:{}", id_prefix))
        .label("Ban")
        .style(ButtonStyle::Danger)
        .emoji('🔨');

    let mut row2_buttons = vec![btn_kick, btn_softban, btn_ban];

    if !is_auto_deleted {
        let btn_del = CreateButton::new(format!("mod_del:{}", id_prefix))
            .label("Delete Msg")
            .style(ButtonStyle::Secondary)
            .emoji('🗑');
        row2_buttons.push(btn_del);
    }

    let row2 = CreateActionRow::Buttons(row2_buttons);
    vec![row1, row2]
}

pub async fn send_mod_alert(
    http: &Arc<Http>,
    mod_channel_id: u64,
    guild_id: Option<GuildId>,
    author_id: UserId,
    author_name: &str,
    channel_id: ChannelId,
    msg_id: MessageId,
    content: &str,
    verdict_title: &str,
    is_auto_deleted: bool,
    action_taken: &str,
    reason: &str,
    score: f64,
    category: &str,
    model_used: &str,
    context_history: &[ChatEntry],
) {
    let gid_str = guild_id.map(|g| g.get().to_string()).unwrap_or_else(|| "0".to_string());
    let color = if is_auto_deleted { COLOR_RED } else { COLOR_ORANGE };

    let primary_embed = CreateEmbed::new()
        .title(verdict_title)
        .color(color)
        .field("Author", format!("<@{}> (`{}` | ID: `{}`)", author_id, author_name, author_id), true)
        .field("Channel", format!("<#{}>", channel_id), true)
        .field("Toxicity Score", format!("{:.2} ({})", score, category), true)
        .field("⚡ Action Taken", action_taken, false)
        .field("AI Detection Pipeline", model_used, false)
        .field("Message Content", format!("```\n{}\n```", if content.len() > 900 { &content[..900] } else { content }), false)
        .field("AI Flag Reason", reason, false)
        .footer(CreateEmbedFooter::new(if is_auto_deleted {
            "Status: AUTO-TIMEOUT APPLIED | Use buttons below to adjust mute time or lift timeout"
        } else {
            "Status: PENDING MOD REVIEW"
        }));

    // Build the 30-message Context Visualizer Embed
    let mut context_text = String::new();
    if context_history.is_empty() {
        context_text.push_str("*(No prior channel context recorded in buffer)*\n");
    } else {
        for (idx, entry) in context_history.iter().enumerate() {
            let line_no = idx + 1;
            let is_target = entry.message_id == msg_id.get();
            let c_clean = entry.content.trim().replace('\n', " ");
            let short_c = if c_clean.len() > 75 { format!("{}...", &c_clean[..75]) } else { c_clean };

            if is_target {
                context_text.push_str(&format!(
                    "🔴 **[{}] @{}:** `{}` ⚠️ **[FLAGGED]**\n",
                    line_no, entry.author_name, short_c
                ));
            } else {
                context_text.push_str(&format!(
                    "`[{}]` **@{}:** {}\n",
                    line_no, entry.author_name, short_c
                ));
            }

            if context_text.len() >= 3800 {
                context_text.push_str("... *(older messages truncated for display)*\n");
                break;
            }
        }
    }

    let context_embed = CreateEmbed::new()
        .title(format!("📜 Channel Context (Last {} Messages)", context_history.len()))
        .color(0x2B2D31)
        .description(context_text);

    let id_prefix = format!("{}:{}:{}:{}", gid_str, author_id.get(), channel_id.get(), msg_id.get());
    let components = create_mod_action_rows(&id_prefix, is_auto_deleted);

    let target_channel = ChannelId::new(mod_channel_id);
    let msg_builder = CreateMessage::new()
        .embed(primary_embed)
        .embed(context_embed)
        .components(components);

    let _ = target_channel.send_message(http, msg_builder).await;
}

pub async fn handle_button_interaction(ctx: &Context, component: &ComponentInteraction) {
    // 1. Check moderator permissions
    let member = match &component.member {
        Some(m) => m,
        None => return,
    };

    let has_mod_perm = member.permissions.map(|p| {
        p.contains(Permissions::ADMINISTRATOR)
            || p.contains(Permissions::MANAGE_MESSAGES)
            || p.contains(Permissions::MODERATE_MEMBERS)
            || p.contains(Permissions::KICK_MEMBERS)
            || p.contains(Permissions::BAN_MEMBERS)
    }).unwrap_or(false);

    if !has_mod_perm {
        let resp = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content("❌ You do not have permission to use moderation controls.")
                .ephemeral(true),
        );
        let _ = component.create_response(&ctx.http, resp).await;
        return;
    }

    // Custom ID: <action>:<guild_id>:<user_id>:<channel_id>:<msg_id>
    let parts: Vec<&str> = component.data.custom_id.split(':').collect();
    if parts.len() < 5 {
        return;
    }

    let action = parts[0];
    let gid_str = parts[1];
    let guild_id = gid_str.parse::<u64>().ok().map(GuildId::new);
    let target_user_id = parts[2].parse::<u64>().ok().map(UserId::new);
    let target_channel_id = parts[3].parse::<u64>().ok().map(ChannelId::new);
    let target_msg_id = parts[4].parse::<u64>().ok().map(MessageId::new);

    let mod_user = &component.user;
    let mut action_status = String::new();

    // Check if action is a timeout change (10m, 1h, 4h, 24h, or legacy 1m)
    let (timeout_dur, timeout_label) = match action {
        "mod_m1m" => (Some(60), "1 minute"),
        "mod_m10" => (Some(600), "10 minutes"),
        "mod_m1h" => (Some(3600), "1 hour"),
        "mod_m4h" => (Some(14400), "4 hours"),
        "mod_m1d" => (Some(86400), "24 hours"),
        _ => (None, ""),
    };

    if let Some(dur) = timeout_dur {
        if let (Some(gid), Some(uid)) = (guild_id, target_user_id) {
            let now_secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
            let until_secs = now_secs + dur;
            if let Ok(ts) = Timestamp::from_unix_timestamp(until_secs) {
                let builder = EditMember::new().disable_communication_until_datetime(ts);
                if gid.edit_member(&ctx.http, uid, builder).await.is_ok() {
                    action_status = format!("⏱️ Mute duration changed to **{}** by <@{}>.", timeout_label, mod_user.id);
                } else {
                    action_status = "❌ Failed to update timeout (missing bot permissions).".to_string();
                }
            }
        }
    } else {
        match action {
            "mod_fp" | "mod_ok" => {
                let mut unmuted = false;
                if let (Some(gid), Some(uid)) = (guild_id, target_user_id) {
                    let now_secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
                    if let Ok(ts_past) = Timestamp::from_unix_timestamp(now_secs - 60) {
                        let builder = EditMember::new().disable_communication_until_datetime(ts_past);
                        if gid.edit_member(&ctx.http, uid, builder).await.is_ok() {
                            unmuted = true;
                        }
                    }
                }
                if unmuted {
                    action_status = format!("✅ **False Positive** confirmed by <@{}>. Timeout lifted and user unmuted!", mod_user.id);
                } else {
                    action_status = format!("✅ Marked as **False Positive** by <@{}>.", mod_user.id);
                }
            }
            "mod_kick" => {
                if let (Some(gid), Some(uid)) = (guild_id, target_user_id) {
                    if gid.kick_with_reason(&ctx.http, uid, "Kicked via AI Mod Alert Button").await.is_ok() {
                        action_status = format!("👢 User was kicked by <@{}>.", mod_user.id);
                    } else {
                        action_status = "❌ Failed to kick user (missing bot permissions or user has higher role).".to_string();
                    }
                }
            }
            "mod_softban" => {
                if let (Some(gid), Some(uid)) = (guild_id, target_user_id) {
                    if gid.ban_with_reason(&ctx.http, uid, 1, "Softbanned via AI Mod Alert Button (Purge 1d)").await.is_ok() {
                        let _ = gid.unban(&ctx.http, uid).await;
                        action_status = format!("🧹 User was softbanned (kicked + messages purged) by <@{}>.", mod_user.id);
                    } else {
                        action_status = "❌ Failed to softban user (missing bot permissions or user has higher role).".to_string();
                    }
                }
            }
            "mod_ban" => {
                if let (Some(gid), Some(uid)) = (guild_id, target_user_id) {
                    if gid.ban_with_reason(&ctx.http, uid, 1, "Banned via AI Mod Alert Button").await.is_ok() {
                        action_status = format!("🔨 User was banned by <@{}>.", mod_user.id);
                    } else {
                        action_status = "❌ Failed to ban user (missing bot permissions or user has higher role).".to_string();
                    }
                }
            }
            "mod_del" => {
                if let (Some(cid), Some(mid)) = (target_channel_id, target_msg_id) {
                    let _ = cid.delete_message(&ctx.http, mid).await;
                    action_status = format!("🗑️ Message deleted from chat by <@{}>.", mod_user.id);
                }
            }
            _ => {}
        }
    }

    // Update embeds while preserving the 30-message context embed
    let mut updated_embeds = Vec::new();
    if let Some(orig_embed) = component.message.embeds.first() {
        let mut updated_primary = CreateEmbed::new();
        if let Some(ref title) = orig_embed.title {
            updated_primary = updated_primary.title(title);
        }
        let color = match action {
            "mod_fp" | "mod_ok" => COLOR_GREEN,
            "mod_m10" | "mod_m1h" | "mod_m4h" | "mod_m1d" | "mod_del" => COLOR_ORANGE,
            _ => COLOR_RED, // kick, softban, ban
        };
        updated_primary = updated_primary.color(color);

        for field in &orig_embed.fields {
            if field.name != "Action Taken" && field.name != "⚡ Action Taken" {
                updated_primary = updated_primary.field(&field.name, &field.value, field.inline);
            }
        }
        updated_primary = updated_primary.field("⚡ Action Taken", &action_status, false);
        updated_primary = updated_primary.footer(CreateEmbedFooter::new(format!("Last action by @{}", mod_user.name)));
        updated_embeds.push(updated_primary);
    }

    // Preserve the second embed (the 30-message context)
    if let Some(orig_context) = component.message.embeds.get(1) {
        let mut updated_context = CreateEmbed::new().color(0x2B2D31);
        if let Some(ref t) = orig_context.title {
            updated_context = updated_context.title(t);
        }
        if let Some(ref d) = orig_context.description {
            updated_context = updated_context.description(d);
        }
        updated_embeds.push(updated_context);
    }

    // If moderator is just updating timeout, keep the buttons so they can re-adjust (e.g. from 1h to 4h) or False Positive!
    // Only remove buttons when an absolute action occurs (False Positive / Kick / Ban / Softban).
    let is_permanent_action = matches!(action, "mod_fp" | "mod_ok" | "mod_kick" | "mod_softban" | "mod_ban");
    let components = if is_permanent_action {
        vec![]
    } else {
        let id_prefix = format!("{}:{}:{}:{}", gid_str, target_user_id.map(|u| u.get()).unwrap_or(0), target_channel_id.map(|c| c.get()).unwrap_or(0), target_msg_id.map(|m| m.get()).unwrap_or(0));
        create_mod_action_rows(&id_prefix, true)
    };

    let mut response_msg = CreateInteractionResponseMessage::new().components(components);
    for embed in updated_embeds {
        response_msg = response_msg.embed(embed);
    }

    let resp = CreateInteractionResponse::UpdateMessage(response_msg);
    let _ = component.create_response(&ctx.http, resp).await;
}
