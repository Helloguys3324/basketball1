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

// Color palettes for mod alerts
const COLOR_RED: u32 = 0xED4245;
const COLOR_ORANGE: u32 = 0xFEE75C;
const COLOR_GREEN: u32 = 0x57F287;

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
    reason: &str,
    score: f64,
    category: &str,
    model_used: &str,
) {
    let gid_str = guild_id.map(|g| g.get().to_string()).unwrap_or_else(|| "0".to_string());
    let color = if is_auto_deleted { COLOR_RED } else { COLOR_ORANGE };

    let embed = CreateEmbed::new()
        .title(verdict_title)
        .color(color)
        .field("Author", format!("<@{}> (`{}` | ID: `{}`)", author_id, author_name, author_id), true)
        .field("Channel", format!("<#{}>", channel_id), true)
        .field("Toxicity Score", format!("{:.2} ({})", score, category), true)
        .field("AI Detection Pipeline", model_used, false)
        .field("Message Content", format!("```\n{}\n```", if content.len() > 900 { &content[..900] } else { content }), false)
        .field("AI Flag Reason", reason, false)
        .footer(CreateEmbedFooter::new(if is_auto_deleted { "Status: AUTO-DELETED from chat" } else { "Status: PENDING MOD REVIEW" }));

    // Buttons:
    // Mute 10m, Mute 1h, Ban, Delete Msg (if not already deleted), Approve/Dismiss
    let id_prefix = format!("{}:{}:{}:{}", gid_str, author_id.get(), channel_id.get(), msg_id.get());

    let btn_mute_10m = CreateButton::new(format!("mod_m10:{}", id_prefix))
        .label("Mute 10m")
        .style(ButtonStyle::Primary)
        .emoji('🔇');

    let btn_mute_1h = CreateButton::new(format!("mod_m1h:{}", id_prefix))
        .label("Mute 1h")
        .style(ButtonStyle::Primary)
        .emoji('⏳');

    let btn_ban = CreateButton::new(format!("mod_ban:{}", id_prefix))
        .label("Ban")
        .style(ButtonStyle::Danger)
        .emoji('🔨');

    let btn_dismiss = CreateButton::new(format!("mod_ok:{}", id_prefix))
        .label("Approve / Dismiss")
        .style(ButtonStyle::Success)
        .emoji('✅');

    let mut action_row = CreateActionRow::Buttons(vec![btn_mute_10m, btn_mute_1h, btn_ban]);

    if !is_auto_deleted {
        let btn_del = CreateButton::new(format!("mod_del:{}", id_prefix))
            .label("Delete Msg")
            .style(ButtonStyle::Secondary)
            .emoji('🗑');
        action_row = CreateActionRow::Buttons(vec![
            CreateButton::new(format!("mod_m10:{}", id_prefix)).label("Mute 10m").style(ButtonStyle::Primary).emoji('🔇'),
            CreateButton::new(format!("mod_m1h:{}", id_prefix)).label("Mute 1h").style(ButtonStyle::Primary).emoji('⏳'),
            CreateButton::new(format!("mod_ban:{}", id_prefix)).label("Ban").style(ButtonStyle::Danger).emoji('🔨'),
            btn_del,
            btn_dismiss,
        ]);
    }

    let target_channel = ChannelId::new(mod_channel_id);
    let msg_builder = CreateMessage::new().embed(embed).components(vec![action_row]);

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
    let guild_id = parts[1].parse::<u64>().ok().map(GuildId::new);
    let target_user_id = parts[2].parse::<u64>().ok().map(UserId::new);
    let target_channel_id = parts[3].parse::<u64>().ok().map(ChannelId::new);
    let target_msg_id = parts[4].parse::<u64>().ok().map(MessageId::new);

    let mod_user = &component.user;
    let mut action_status = String::new();

    match action {
        "mod_m10" => {
            if let (Some(gid), Some(uid)) = (guild_id, target_user_id) {
                let now_secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
                let until_secs = now_secs + 600; // 10 minutes
                if let Ok(ts) = Timestamp::from_unix_timestamp(until_secs) {
                    let builder = EditMember::new().disable_communication_until_datetime(ts);
                    if gid.edit_member(&ctx.http, uid, builder).await.is_ok() {
                        action_status = format!("🔇 User timed out for 10 minutes by <@{}>.", mod_user.id);
                    } else {
                        action_status = "❌ Failed to time out user (missing bot permissions).".to_string();
                    }
                }
            }
        }
        "mod_m1h" => {
            if let (Some(gid), Some(uid)) = (guild_id, target_user_id) {
                let now_secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
                let until_secs = now_secs + 3600; // 1 hour
                if let Ok(ts) = Timestamp::from_unix_timestamp(until_secs) {
                    let builder = EditMember::new().disable_communication_until_datetime(ts);
                    if gid.edit_member(&ctx.http, uid, builder).await.is_ok() {
                        action_status = format!("⏳ User timed out for 1 hour by <@{}>.", mod_user.id);
                    } else {
                        action_status = "❌ Failed to time out user (missing bot permissions).".to_string();
                    }
                }
            }
        }
        "mod_ban" => {
            if let (Some(gid), Some(uid)) = (guild_id, target_user_id) {
                if gid.ban_with_reason(&ctx.http, uid, 0, "Banned via AI Mod Alert Button").await.is_ok() {
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
        "mod_ok" => {
            action_status = format!("✅ Approved as false positive / dismissed by <@{}>.", mod_user.id);
        }
        _ => {}
    }

    // Update the embed and disable buttons
    let mut updated_embed = CreateEmbed::new();
    if let Some(orig_embed) = component.message.embeds.first() {
        if let Some(ref title) = orig_embed.title {
            updated_embed = updated_embed.title(title);
        }
        let color = if action == "mod_ok" { COLOR_GREEN } else { COLOR_RED };
        updated_embed = updated_embed.color(color);

        for field in &orig_embed.fields {
            updated_embed = updated_embed.field(&field.name, &field.value, field.inline);
        }
        updated_embed = updated_embed.field("Action Taken", &action_status, false);
        updated_embed = updated_embed.footer(CreateEmbedFooter::new(format!("Resolved by {}", mod_user.name)));
    }

    let resp = CreateInteractionResponse::UpdateMessage(
        CreateInteractionResponseMessage::new()
            .embed(updated_embed)
            .components(vec![]), // Removes buttons once action is performed
    );

    let _ = component.create_response(&ctx.http, resp).await;
}
