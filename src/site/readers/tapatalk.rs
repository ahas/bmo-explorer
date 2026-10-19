//! A forum read through its Tapatalk plugin as a site tool (docs/site-contract.md, 「Where a tool lives」). The plugin
//! is the one API the Tapatalk app reads vBulletin, XenForo, phpBB and their kin through: one XML-RPC endpoint
//! under the forum's own address, which answers a guest where the forum's pages stand behind a challenge. This
//! reads that endpoint and no page of the forum.
//!
//! The reading is the same for every such forum; which forum a tool reads, the path it stands under, the
//! addresses of its own pages, what the tool is called and when it was made are the tool's own, declared by the
//! module that names it with `tapatalk_forum!`. Which topic, post or board an address names is asked of the
//! plugin, so no forum engine's address forms are read here. The plugin's answer is read by an XML-RPC reader.

use std::collections::BTreeMap;

use crate::forms::{Failure, WebError, WebResult};
use crate::tool::NonEmpty;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use serde_xmlrpc::Value;
use url::Url;

use super::{take_units, units};
use crate::site::contract::{PageRequest, Reach};

/// One forum a tool reads: its origin, the path the forum and its plugin stand under, the name the forum goes by,
/// and the addresses of its own pages, which only the forum's engine knows the form of.
pub struct TapatalkForum {
  pub origin: &'static str,
  pub base: &'static str,
  pub forum_name: &'static str,
  pub topic_page: fn(&str) -> String,
  pub post_page: fn(&str) -> String,
  pub board_page: fn(&str) -> String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attachment {
  pub name: String,
  pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TapatalkPost {
  pub position: i64,
  pub id: String,
  pub author: String,
  pub author_type: String,
  pub time: Option<DateTime<Utc>>,
  pub editor: Option<String>,
  pub edited: Option<DateTime<Utc>>,
  pub edit_reason: Option<String>,
  pub approved: bool,
  pub text: String,
  pub attachments: Vec<Attachment>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TapatalkTopic {
  pub id: String,
  pub title: String,
  pub boards: Vec<String>,
  pub posts: i64,
  pub views: Option<i64>,
  pub started: Option<String>,
  pub page: Vec<TapatalkPost>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TapatalkBoardTopic {
  pub id: String,
  pub title: String,
  pub author: String,
  pub replies: i64,
  pub views: Option<i64>,
  pub last_reply: Option<DateTime<Utc>>,
  pub last_replier: Option<String>,
  pub opening: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TapatalkBoard {
  pub id: String,
  pub name: String,
  pub topics: i64,
  pub page: Vec<TapatalkBoardTopic>,
}

/// What the plugin says an address names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TapatalkIds {
  pub topic: Option<String>,
  pub post: Option<String>,
  pub board: Option<String>,
}

/// Why the plugin's answer could not be had: the plugin's own refusal — a topic it does not show a guest, a call
/// a guest may not make — in its own words, or a failure to reach or read it.
pub enum Asked {
  Refused(String),
  Failed(WebError),
}

/// The most posts the plugin hands over in one call, which it fixes, and the topics one board answer lists.
pub const POSTS_PER_CALL: i64 = 50;
pub const BOARD_TOPICS: i64 = 20;

// One answer's room for the posts, under the ceiling every answer of ours stands under.
const POST_BUDGET: i64 = 20_000;

pub fn endpoint_of(forum: &TapatalkForum) -> String {
  format!("{}{}/mobiquo/mobiquo.php", forum.origin, forum.base)
}

fn call(method: &str, params: Vec<Value>) -> PageRequest {
  PageRequest {
    body: serde_xmlrpc::request_to_string(method, params.into_iter()).ok(),
    headers: vec![("content-type".into(), "text/xml".into())],
  }
}

fn int(value: i64) -> Value {
  Value::Int(i32::try_from(value).unwrap_or(i32::MAX))
}

pub fn ids_request(url: &str) -> PageRequest {
  call("get_id_by_url", vec![Value::String(url.to_owned())])
}

/// A topic's posts from a position, counted from one as the forum counts them.
pub fn topic_request(topic: &str, from: i64) -> PageRequest {
  call(
    "get_thread",
    vec![
      Value::String(topic.to_owned()),
      int(from - 1),
      int(from - 2 + POSTS_PER_CALL),
      Value::Bool(false),
    ],
  )
}

pub fn post_request(post: &str) -> PageRequest {
  call(
    "get_thread_by_post",
    vec![Value::String(post.to_owned()), int(POSTS_PER_CALL), Value::Bool(false)],
  )
}

pub fn board_request(board: &str, from: i64) -> PageRequest {
  call(
    "get_topic",
    vec![Value::String(board.to_owned()), int(from - 1), int(from - 2 + BOARD_TOPICS)],
  )
}

type Struct = BTreeMap<String, Value>;

/// The plugin's answer as a struct; a fault fails, and a struct saying the call was refused is the refusal.
pub fn parse_answer(body: &str) -> Result<Struct, Asked> {
  let value: Value = match serde_xmlrpc::response_from_str(body) {
    Ok(value) => value,
    Err(serde_xmlrpc::Error::Fault(fault)) => {
      return Err(Asked::Failed(WebError::new(
        format!("the forum's Tapatalk plugin answered a fault: {fault}"),
        Failure::Http,
      )));
    }
    Err(_) => {
      return Err(Asked::Failed(WebError::new(
        "the forum's Tapatalk plugin answered nothing readable as XML-RPC",
        Failure::Http,
      )));
    }
  };
  let answer = match value {
    Value::Struct(answer) => answer,
    _ => Struct::new(),
  };

  if matches!(answer.get("result"), Some(Value::Bool(false) | Value::Int(0)))
    || matches!(answer.get("result"), Some(Value::String(result)) if result == "0")
  {
    return Err(Asked::Refused(
      text(answer.get("result_text")).unwrap_or_else(|| "the forum refused the request".into()),
    ));
  }

  Ok(answer)
}

fn structs(value: Option<&Value>) -> Vec<&Struct> {
  match value {
    Some(Value::Array(items)) => items
      .iter()
      .filter_map(|item| if let Value::Struct(one) = item { Some(one) } else { None })
      .collect(),
    _ => Vec::new(),
  }
}

// The plugin sends a forum's own words as base64, decoded to bytes, and its names and ids as strings.
fn text(value: Option<&Value>) -> Option<String> {
  let read = match value? {
    Value::Base64(bytes) => String::from_utf8_lossy(bytes).into_owned(),
    Value::String(text) => text.clone(),
    Value::Int(number) => number.to_string(),
    Value::Int64(number) => number.to_string(),
    Value::Double(number) => number.to_string(),
    _ => String::new(),
  };
  let read = read.trim();

  (!read.is_empty()).then(|| read.to_owned())
}

// A count, sent as an int or as its digits.
fn count(value: Option<&Value>) -> Option<i64> {
  match value? {
    Value::Int(number) => Some(i64::from(*number)),
    Value::Int64(number) => Some(*number),
    Value::Double(number) if number.is_finite() => Some(*number as i64),
    Value::String(digits)
      if !digits.trim().is_empty() && digits.trim().bytes().all(|byte| byte.is_ascii_digit()) =>
    {
      digits.trim().parse().ok()
    }
    _ => None,
  }
}

// A time, sent as a date with its offset or as seconds since the epoch.
fn time(value: Option<&Value>) -> Option<DateTime<Utc>> {
  if let Some(Value::DateTime(at)) = value {
    return DateTime::parse_from_rfc3339(&at.to_string()).ok().map(|at| at.with_timezone(&Utc));
  }

  count(value)
    .filter(|seconds| *seconds != 0)
    .and_then(|seconds| DateTime::from_timestamp(seconds, 0))
}

fn flag(value: Option<&Value>) -> bool {
  matches!(value, Some(Value::Bool(true) | Value::Int(1)))
    || matches!(value, Some(Value::String(one)) if one == "1")
}

pub fn parse_ids(answer: &Struct) -> TapatalkIds {
  TapatalkIds {
    topic: text(answer.get("topic_id")),
    post: text(answer.get("post_id")),
    board: text(answer.get("forum_id")),
  }
}

fn parse_post(post: &Struct, position: i64) -> TapatalkPost {
  let mut attachments: Vec<Attachment> = Vec::new();

  for file in structs(post.get("attachments"))
    .into_iter()
    .chain(structs(post.get("inlineattachments")))
  {
    let Some(url) = text(file.get("url")) else {
      continue;
    };
    let name = text(file.get("filename")).unwrap_or_else(|| "attachment".into());

    // One file attached twice is named once, by its last listing, in the place it was first listed.
    match attachments.iter_mut().find(|one| one.url == url) {
      Some(one) => one.name = name,
      None => attachments.push(Attachment { name, url }),
    }
  }

  TapatalkPost {
    position: count(post.get("post_count")).unwrap_or(position),
    id: text(post.get("post_id")).unwrap_or_default(),
    author: text(post.get("post_author_name")).unwrap_or_else(|| "unknown".into()),
    author_type: text(post.get("post_author_user_type")).unwrap_or_else(|| "normal".into()),
    time: time(post.get("post_time")).or_else(|| time(post.get("timestamp"))),
    editor: text(post.get("editor_name")),
    edited: time(post.get("edit_time")),
    edit_reason: text(post.get("edit_reason")),
    approved: post.get("is_approved").is_none() || flag(post.get("is_approved")),
    text: text(post.get("post_content")).unwrap_or_default(),
    attachments,
  }
}

/// A topic's posts as `get_thread` answers them.
pub fn parse_topic(answer: &Struct, from: i64) -> TapatalkTopic {
  let posts: Vec<TapatalkPost> = structs(answer.get("posts"))
    .into_iter()
    .enumerate()
    .map(|(index, post)| parse_post(post, from + index as i64))
    .collect();

  TapatalkTopic {
    id: text(answer.get("topic_id")).unwrap_or_default(),
    title: text(answer.get("topic_title")).unwrap_or_else(|| "(untitled)".into()),
    boards: structs(answer.get("breadcrumb"))
      .into_iter()
      .filter_map(|board| text(board.get("forum_name")))
      .collect(),
    posts: count(answer.get("total_post_num")).unwrap_or(posts.len() as i64),
    views: count(answer.get("view_number")),
    started: text(answer.get("topic_author_name")),
    page: posts,
  }
}

/// Where a post stands in its topic, as `get_thread_by_post` answers.
pub fn parse_post_place(answer: &Struct) -> Option<(String, i64)> {
  let topic = text(answer.get("topic_id"))?;
  let position = count(answer.get("position")).filter(|position| *position >= 1)?;

  Some((topic, position))
}

pub fn parse_board(answer: &Struct) -> TapatalkBoard {
  TapatalkBoard {
    id: text(answer.get("forum_id")).unwrap_or_default(),
    name: text(answer.get("forum_name")).unwrap_or_else(|| "(unnamed board)".into()),
    topics: count(answer.get("total_topic_num")).unwrap_or(0),
    page: structs(answer.get("topics"))
      .into_iter()
      .filter_map(|topic| {
        Some(TapatalkBoardTopic {
          id: text(topic.get("topic_id"))?,
          title: text(topic.get("topic_title")).unwrap_or_else(|| "(untitled)".into()),
          author: text(topic.get("topic_author_name")).unwrap_or_else(|| "unknown".into()),
          replies: count(topic.get("reply_number")).unwrap_or(0),
          views: count(topic.get("view_number")),
          last_reply: time(topic.get("last_reply_time")).or_else(|| time(topic.get("timestamp"))),
          last_replier: text(topic.get("last_reply_user")),
          opening: text(topic.get("short_content")),
        })
      })
      .collect(),
  }
}

// A time as the minute it names, in UTC.
fn minute(at: Option<DateTime<Utc>>) -> String {
  at.map_or_else(|| "unknown".into(), |at| format!("{} UTC", at.format("%Y-%m-%d %H:%M")))
}

fn post_head(post: &TapatalkPost) -> String {
  let mut parts = vec![
    format!("### #{}", post.position),
    if post.author_type == "normal" {
      post.author.clone()
    } else {
      format!("{} ({})", post.author, post.author_type)
    },
    minute(post.time),
  ];

  if let Some(editor) = &post.editor {
    let when = post.edited.map_or_else(String::new, |edited| format!(" {}", minute(Some(edited))));
    let why = post.edit_reason.as_ref().map_or_else(String::new, |reason| format!(": {reason}"));

    parts.push(format!("edited by {editor}{when}{why}"));
  }

  if !post.approved {
    parts.push("awaiting moderation".into());
  }

  parts.join(" · ")
}

fn post_body(post: &TapatalkPost) -> String {
  if post.attachments.is_empty() {
    return post.text.clone();
  }

  format!(
    "{}\n\nAttached: {}",
    post.text,
    post
      .attachments
      .iter()
      .map(|file| format!("{} ({})", file.name, file.url))
      .collect::<Vec<_>>()
      .join(", ")
  )
}

/// A topic's posts from the one asked for, as many as one answer holds, saying where the next answer starts where
/// the topic goes on past them.
pub fn render_topic(forum: &TapatalkForum, topic: &TapatalkTopic) -> String {
  let mut blocks = Vec::new();
  let mut used: i64 = 0;
  let mut next = None;

  for (index, post) in topic.page.iter().enumerate() {
    let head = post_head(post);
    let body = post_body(post);
    let room = POST_BUDGET - used - units(&head) as i64;

    if index > 0 && units(&body) as i64 > room {
      next = Some(post.position);
      break;
    }

    let shown = if units(&body) as i64 > room {
      format!(
        "{}\n… (the post goes on; it is cut here at the answer's length)",
        take_units(&body, room.max(2_000) as usize)
      )
    } else {
      body
    };

    used += (units(&head) + units(&shown)) as i64;
    blocks.push(format!("{head}\n\n{shown}"));
  }

  let shown = &topic.page[..blocks.len()];
  let first = shown.first();
  let last = shown.last().map(|post| post.position);

  if next.is_none()
    && let Some(last) = last
    && last < topic.posts
  {
    next = Some(last + 1);
  }

  let source = match first {
    Some(first) if first.position > 1 => (forum.post_page)(&first.id),
    _ => (forum.topic_page)(&topic.id),
  };
  let mut lines = vec![
    format!("SOURCE PAGE: {source} ({}, read from the forum's Tapatalk API)", forum.forum_name),
    format!("Title: {}", topic.title),
  ];

  if !topic.boards.is_empty() {
    lines.push(format!("Board: {}", topic.boards.join(" › ")));
  }

  lines.push(format!(
    "{}{} posts{}",
    topic
      .started
      .as_ref()
      .map_or_else(String::new, |started| format!("Started by {started} · ")),
    topic.posts,
    topic.views.map_or_else(String::new, |views| format!(" · {views} views"))
  ));
  lines.push(match (first, last) {
    (Some(first), Some(last)) => format!(
      "POSTS #{}–#{last} of {}, each as its author wrote it in the forum's BBCode:",
      first.position, topic.posts
    ),
    _ => "No post of this topic stands at that position or after it.".into(),
  });
  lines.push(String::new());

  for block in blocks {
    lines.push(block);
    lines.push(String::new());
  }

  lines.push(next.map_or_else(
    || "End of the topic.".into(),
    |next| format!("MORE: the topic goes on at #{next}; ask again with from {next}."),
  ));
  lines.push(
    "Posts are what the forum's members wrote; they are owners' experience, not the maker's documentation."
      .into(),
  );
  lines.join("\n")
}

pub fn render_board(forum: &TapatalkForum, board: &TapatalkBoard, from: i64) -> String {
  let last = from + board.page.len() as i64 - 1;
  let mut lines = vec![
    format!(
      "SOURCE PAGE: {} ({} board, read from the forum's Tapatalk API)",
      (forum.board_page)(&board.id),
      forum.forum_name
    ),
    format!("Board: {} · {} topics", board.name, board.topics),
    if board.page.is_empty() {
      "No topic of this board stands at that position or after it.".into()
    } else {
      format!("TOPICS {from}–{last}, newest reply first (pinned topics aside):")
    },
  ];

  for topic in &board.page {
    let mut head = vec![
      format!("- {}", topic.title),
      format!("by {}", topic.author),
      format!("{} replies", topic.replies),
    ];

    if let Some(views) = topic.views {
      head.push(format!("{views} views"));
    }

    head.push(format!(
      "last reply {}{}",
      minute(topic.last_reply),
      topic.last_replier.as_ref().map_or_else(String::new, |replier| format!(" by {replier}"))
    ));
    head.push((forum.topic_page)(&topic.id));

    let head = head.join(" · ");

    lines.push(match &topic.opening {
      None => head,
      Some(opening) => format!("{head}\n  {opening}"),
    });
  }

  lines.push(if !board.page.is_empty() && last < board.topics {
    format!("MORE: the board goes on at topic {}; ask again with from {}.", last + 1, last + 1)
  } else {
    "End of the board.".into()
  });
  lines.push("Ask again with a topic's address for its posts.".into());
  lines.join("\n")
}

async fn ask(forum: &TapatalkForum, request: PageRequest, reach: &dyn Reach) -> Result<Struct, Asked> {
  let body = reach.page(&endpoint_of(forum), Some(request)).await.map_err(Asked::Failed)?;

  parse_answer(&body)
}

fn refused(forum: &TapatalkForum, page: &str, what: &str, asked: Asked) -> WebResult<String> {
  match asked {
    Asked::Refused(why) => Ok(format!(
      "SOURCE PAGE: {page} ({})\n{} does not show a guest {what}: {why}",
      forum.forum_name, forum.forum_name
    )),
    Asked::Failed(error) => Err(error),
  }
}

pub async fn read_topic(
  forum: &TapatalkForum,
  topic: &str,
  from: i64,
  reach: &dyn Reach,
) -> WebResult<String> {
  match ask(forum, topic_request(topic, from), reach).await {
    Ok(answer) => Ok(render_topic(forum, &parse_topic(&answer, from))),
    Err(asked) => refused(forum, &(forum.topic_page)(topic), &format!("topic {topic}"), asked),
  }
}

pub async fn read_post(
  forum: &TapatalkForum,
  post: &str,
  from: Option<i64>,
  reach: &dyn Reach,
) -> WebResult<String> {
  let place = match ask(forum, post_request(post), reach).await {
    Ok(answer) => parse_post_place(&answer),
    Err(asked) => return refused(forum, &(forum.post_page)(post), &format!("post {post}"), asked),
  };
  let Some((topic, position)) = place else {
    return Ok(format!(
      "{} UNAVAILABLE: the forum did not say which topic post {post} stands in.",
      forum.forum_name.to_uppercase()
    ));
  };

  read_topic(forum, &topic, from.unwrap_or(position), reach).await
}

pub async fn read_board(
  forum: &TapatalkForum,
  board: &str,
  from: i64,
  reach: &dyn Reach,
) -> WebResult<String> {
  match ask(forum, board_request(board, from), reach).await {
    Ok(answer) => Ok(render_board(forum, &parse_board(&answer), from)),
    Err(asked) => refused(forum, &(forum.board_page)(board), &format!("board {board}"), asked),
  }
}

fn bare_host(host: &str) -> String {
  let host = host.to_lowercase();

  host.strip_prefix("www.").map_or_else(|| host.clone(), str::to_owned)
}

/// Whether an address is one of the forum's own pages: under the forum's path on its host, with or without
/// `www.`, and not the plugin's own endpoint.
pub fn forum_address(url: &Url, forum: &TapatalkForum) -> bool {
  let path = url.path().to_lowercase();
  let base = forum.base.to_lowercase();
  let origin_host = Url::parse(forum.origin)
    .ok()
    .and_then(|origin| origin.host_str().map(bare_host))
    .unwrap_or_default();

  matches!(url.scheme(), "http" | "https")
    && url.host_str().map(bare_host).as_deref() == Some(origin_host.as_str())
    && (path == base || path.starts_with(&format!("{base}/")))
    && !path.starts_with(&format!("{base}/mobiquo/"))
}

/// Whatever an address names, as the plugin reads it.
pub async fn read_address(
  forum: &TapatalkForum,
  url: &str,
  from: Option<i64>,
  reach: &dyn Reach,
) -> WebResult<String> {
  let ids = match ask(forum, ids_request(url), reach).await {
    Ok(answer) => parse_ids(&answer),
    Err(asked) => return refused(forum, url, "what this address names", asked),
  };

  if let Some(post) = ids.post {
    return read_post(forum, &post, from, reach).await;
  }

  if let Some(topic) = ids.topic {
    return read_topic(forum, &topic, from.unwrap_or(1), reach).await;
  }

  if let Some(board) = ids.board {
    return read_board(forum, &board, from.unwrap_or(1), reach).await;
  }

  Ok(format!(
    "SOURCE PAGE: {url} ({})\nThe forum names no topic, post or board at this address; give a topic's, a post's or a board's address.",
    forum.forum_name
  ))
}

#[derive(Deserialize, JsonSchema)]
pub struct TapatalkInput {
  /// A topic's, a post's or a board's address on the forum, or a topic's number.
  query: NonEmpty,
  /// Where to read from, where an answer said it goes on: a topic's post position, or a board's topic position (default: the post an address names, else 1).
  #[schemars(range(min = 1))]
  from: Option<u32>,
}

/// A forum tool's arguments for an address it claims: the address itself.
pub fn tapatalk_claim(forum: &TapatalkForum, url: &Url) -> Option<serde_json::Value> {
  forum_address(url, forum).then(|| json!({ "query": url.as_str() }))
}

pub async fn run_tapatalk(
  forum: &TapatalkForum,
  input: TapatalkInput,
  reach: &dyn Reach,
) -> WebResult<String> {
  let given = input.query.trim();
  let from = input.from.map(i64::from);

  if !given.is_empty() && given.bytes().all(|byte| byte.is_ascii_digit()) {
    return read_topic(forum, given, from.unwrap_or(1), reach).await;
  }

  let is_web = given.get(..7).is_some_and(|head| head.eq_ignore_ascii_case("http://"))
    || given.get(..8).is_some_and(|head| head.eq_ignore_ascii_case("https://"));
  let address = Url::parse(given).ok().filter(|url| is_web && forum_address(url, forum));

  if address.is_none() {
    let host = Url::parse(forum.origin)
      .ok()
      .and_then(|origin| origin.host_str().map(str::to_lowercase))
      .unwrap_or_default();

    return Err(WebError::new(
      format!(
        "{given} is not a {} address or topic number ({host}{}/…); the forum lets no guest search it, so find a topic with the web search narrowed to site:{}",
        forum.forum_name,
        forum.base,
        bare_host(&host)
      ),
      Failure::Transport,
    ));
  }

  read_address(forum, given, from, reach).await
}
