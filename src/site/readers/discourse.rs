//! A Discourse forum read as a site tool (docs/site-contract.md, 「Where a tool lives」). Discourse draws its topic
//! pages by script, so a plain fetch of one carries no posts, while every forum serves the same public views to
//! anyone: a topic's JSON with its title, tags, counts and first posts, a topic's posts as their authors wrote
//! them in markdown under `/raw/<topic>`, a hundred to a page, and its search as JSON. This reads those three
//! and no page's markup.
//!
//! The reading is the same for every Discourse forum; which forum a tool reads, what it is called and when it
//! was made are the tool's own, declared by the module that names the forum with `discourse_forum!`.

use std::collections::{BTreeMap, HashSet};
use std::sync::LazyLock;

use crate::forms::{Failure, WebError, WebResult};
use crate::tool::NonEmpty;
use regex::Regex;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use url::Url;

use super::markup::decode_entities;
use super::{take_units, units, url_component};
use crate::site::contract::{Reach, status_of};

/// One forum a tool reads: its origin and the name the forum goes by.
pub struct DiscourseForum {
  pub origin: &'static str,
  pub forum_name: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForumPostMeta {
  pub created: String,
  pub staff: bool,
  pub reply_to: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Solution {
  pub post: i64,
  pub username: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForumTopic {
  pub id: i64,
  pub slug: String,
  pub title: String,
  pub tags: Vec<String>,
  pub created: Option<String>,
  pub last_posted: Option<String>,
  pub posts: i64,
  pub views: Option<i64>,
  pub closed: bool,
  pub archived: bool,
  pub solution: Option<Solution>,
}

/// One post as the raw view gives it: the time it shows is the post's last edit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawPost {
  pub number: i64,
  pub username: String,
  pub edited: String,
  pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForumHit {
  pub topic: i64,
  pub slug: String,
  pub title: String,
  pub posts: i64,
  pub created: Option<String>,
  pub last_posted: Option<String>,
  pub tags: Vec<String>,
  pub solved: bool,
  pub post: Option<i64>,
  pub blurb: Option<String>,
}

/// What one address or one argument asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForumRequest {
  Topic { topic: i64, post: i64 },
  Search { query: String },
}

/// The raw view's page size, which Discourse fixes.
pub const RAW_PAGE_SIZE: usize = 100;

// One answer's room for the posts, under the ceiling every answer of ours stands under.
const POST_BUDGET: i64 = 20_000;

// How far the reading steps back when deleted posts have moved a post number onto an earlier raw page, and how
// many further pages it reads to fill one answer.
const STEPS_BACK: usize = 3;
const PAGES_ON: usize = 2;
const SEARCH_LIMIT: usize = 10;

fn host_of(origin: &str) -> String {
  Url::parse(origin)
    .ok()
    .and_then(|url| url.host_str().map(str::to_lowercase))
    .unwrap_or_default()
}

fn positive(text: Option<&str>) -> Option<i64> {
  text
    .map(str::trim)
    .filter(|text| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()))
    .and_then(|text| text.parse().ok())
    .filter(|value| *value > 0)
}

/// What an address on the forum asks for: a topic as `/t/<slug>/<id>`, `/t/<id>` or `/raw/<id>`, each with a post
/// number after it or not, or a search as `/search?q=…`. None where it is neither.
pub fn forum_request_of(url: &Url, origin: &str) -> Option<ForumRequest> {
  if url.host_str().map(str::to_lowercase) != Some(host_of(origin)) {
    return None;
  }

  let path = url.path().strip_suffix(".json").unwrap_or(url.path());

  if path.strip_suffix('/').unwrap_or(path) == "/search" {
    let words = url
      .query_pairs()
      .find(|(key, _)| key == "q")
      .map(|(_, value)| value.trim().to_owned())
      .unwrap_or_default();

    return (!words.is_empty()).then_some(ForumRequest::Search { query: words });
  }

  let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
  let (kind, rest) = parts.split_first()?;

  if *kind == "raw" && (1..=2).contains(&rest.len()) {
    let topic = positive(rest.first().copied())?;

    return Some(ForumRequest::Topic {
      topic,
      post: positive(rest.get(1).copied()).unwrap_or(1),
    });
  }

  if *kind != "t" || rest.is_empty() || rest.len() > 3 {
    return None;
  }

  // `/t/<id>/<post>` and `/t/<slug>/<id>` both have two parts; a slug is never digits alone.
  let by_slug = rest.len() == 3 || (rest.len() == 2 && positive(rest.first().copied()).is_none());
  let (topic_at, post_at) = if by_slug { (1, 2) } else { (0, 1) };
  let topic = positive(rest.get(topic_at).copied())?;

  Some(ForumRequest::Topic {
    topic,
    post: positive(rest.get(post_at).copied()).unwrap_or(1),
  })
}

pub fn topic_json_url(origin: &str, topic: i64) -> String {
  format!("{origin}/t/{topic}.json")
}

/// The twenty posts after a post number, with what the raw view leaves out: when each was first posted, whether
/// its author is the forum's staff, and which post it answers.
pub fn posts_json_url(origin: &str, topic: i64, after: i64) -> String {
  format!("{origin}/t/{topic}/posts.json?post_number={after}&asc=true")
}

pub fn raw_url(origin: &str, topic: i64, page: usize) -> String {
  format!("{origin}/raw/{topic}?page={page}")
}

pub fn search_json_url(origin: &str, query: &str) -> String {
  format!("{origin}/search.json?q={}", url_component(query))
}

fn parsed(body: &str) -> Option<Map<String, Value>> {
  match serde_json::from_str(body) {
    Ok(Value::Object(object)) => Some(object),
    _ => None,
  }
}

fn text(value: Option<&Value>) -> Option<String> {
  value
    .and_then(Value::as_str)
    .map(str::trim)
    .filter(|text| !text.is_empty())
    .map(str::to_owned)
}

fn count(value: Option<&Value>) -> Option<i64> {
  value.and_then(|value| {
    value
      .as_i64()
      .or_else(|| value.as_f64().filter(|number| number.fract() == 0.0).map(|number| number as i64))
  })
}

fn records(value: Option<&Value>) -> Vec<&Map<String, Value>> {
  value
    .and_then(Value::as_array)
    .map(|items| items.iter().filter_map(Value::as_object).collect())
    .unwrap_or_default()
}

fn is_true(value: Option<&Value>) -> bool {
  value == Some(&Value::Bool(true))
}

// A forum names a tag either by its name alone or as an object carrying it, by its version.
fn tags_of(value: Option<&Value>) -> Vec<String> {
  value
    .and_then(Value::as_array)
    .map(|tags| {
      tags
        .iter()
        .filter_map(|tag| text(Some(tag)).or_else(|| text(tag.get("name"))))
        .collect()
    })
    .unwrap_or_default()
}

/// The topic as its JSON answers; none where the body is not a topic.
pub fn parse_topic(body: &str) -> Option<ForumTopic> {
  let topic = parsed(body)?;
  let id = count(topic.get("id"))?;
  let title = text(topic.get("title"))?;
  let accepted = topic.get("accepted_answer");
  let posts = records(topic.get("post_stream").and_then(|stream| stream.get("posts")));
  let accepted_post = count(accepted.and_then(|accepted| accepted.get("post_number"))).or_else(|| {
    count(
      posts
        .iter()
        .find(|post| is_true(post.get("accepted_answer")))
        .and_then(|post| post.get("post_number")),
    )
  });

  Some(ForumTopic {
    id,
    slug: text(topic.get("slug")).unwrap_or_else(|| "topic".into()),
    title,
    tags: tags_of(topic.get("tags")),
    created: text(topic.get("created_at")),
    last_posted: text(topic.get("last_posted_at")),
    posts: count(topic.get("posts_count")).unwrap_or(0),
    views: count(topic.get("views")),
    closed: is_true(topic.get("closed")),
    archived: is_true(topic.get("archived")),
    solution: accepted_post.map(|post| Solution {
      post,
      username: text(accepted.and_then(|accepted| accepted.get("username")))
        .or_else(|| {
          text(
            posts
              .iter()
              .find(|one| count(one.get("post_number")) == Some(post))
              .and_then(|one| one.get("username")),
          )
        })
        .unwrap_or_default(),
    }),
  })
}

/// What a topic's or a posts' JSON says of each post it carries, by post number.
pub fn parse_post_meta(body: &str) -> BTreeMap<i64, ForumPostMeta> {
  let Some(answer) = parsed(body) else {
    return BTreeMap::new();
  };

  records(answer.get("post_stream").and_then(|stream| stream.get("posts")))
    .into_iter()
    .filter_map(|post| {
      let number = count(post.get("post_number"))?;
      let created = text(post.get("created_at"))?;

      Some((
        number,
        ForumPostMeta {
          created,
          staff: is_true(post.get("staff")),
          reply_to: count(post.get("reply_to_post_number")),
        },
      ))
    })
    .collect()
}

const DASHES: &str = "-------------------------";
static RAW_OPENING: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"^(\S+) \| (\d{4}-\d\d-\d\d \d\d:\d\d):\d\d UTC \| #(\d+)$").expect("a fixed pattern")
});

/// The raw view's posts: each opens on `<username> | <time> UTC | #<number>` and closes on a line of twenty-five
/// dashes. A line of dashes inside a post is not a close unless a post's opening line follows it, so a post that
/// draws one keeps it.
pub fn parse_raw(body: &str) -> Vec<RawPost> {
  struct Open {
    number: i64,
    username: String,
    edited: String,
    lines: Vec<String>,
  }

  let mut posts = Vec::new();
  let mut current: Option<Open> = None;
  let mut previous: Option<String> = None;
  let close = |current: Option<Open>, posts: &mut Vec<RawPost>| {
    let Some(open) = current else {
      return;
    };
    let mut kept = open.lines;

    while kept.last().is_some_and(|last| last.trim().is_empty() || last == DASHES) {
      kept.pop();
    }

    posts.push(RawPost {
      number: open.number,
      username: open.username,
      edited: format!("{} UTC", open.edited),
      text: kept.join("\n").trim().to_owned(),
    });
  };

  for line in body.split('\n') {
    if let Some(head) = RAW_OPENING.captures(line)
      && (current.is_none() || previous.as_deref() == Some(DASHES))
    {
      close(current.take(), &mut posts);
      current = Some(Open {
        number: head[3].parse().unwrap_or(0),
        username: head[1].to_owned(),
        edited: head[2].to_owned(),
        lines: Vec::new(),
      });
      previous = None;
      continue;
    }

    if let Some(open) = current.as_mut() {
      open.lines.push(line.to_owned());
    }

    if !line.trim().is_empty() {
      previous = Some(line.to_owned());
    }
  }

  close(current, &mut posts);
  posts
}

/// The forum's search as its JSON answers: each matching post's topic once, in the forum's order.
pub fn parse_search(body: &str) -> Option<Vec<ForumHit>> {
  let answer = parsed(body)?;
  let topics: Vec<(i64, &Map<String, Value>)> = records(answer.get("topics"))
    .into_iter()
    .filter_map(|topic| count(topic.get("id")).map(|id| (id, topic)))
    .collect();
  let posts = records(answer.get("posts"));
  let mut seen = HashSet::new();
  let order = posts
    .iter()
    .filter_map(|post| count(post.get("topic_id")))
    .chain(topics.iter().map(|(id, _)| *id))
    .filter(|id| seen.insert(*id));
  let topic_of = |id: i64| topics.iter().rev().find(|(one, _)| *one == id).map(|(_, topic)| *topic);

  Some(
    order
      .collect::<Vec<_>>()
      .into_iter()
      .filter_map(|id| {
        let topic = topic_of(id)?;
        let title = text(topic.get("title"))?;
        let post = posts.iter().find(|one| count(one.get("topic_id")) == Some(id));

        Some(ForumHit {
          topic: id,
          slug: text(topic.get("slug")).unwrap_or_else(|| "topic".into()),
          title,
          posts: count(topic.get("posts_count")).unwrap_or(0),
          created: text(topic.get("created_at")),
          last_posted: text(topic.get("last_posted_at")),
          tags: tags_of(topic.get("tags")),
          solved: is_true(topic.get("has_accepted_answer")),
          post: count(post.and_then(|post| post.get("post_number"))),
          blurb: plain(text(post.and_then(|post| post.get("blurb")))),
        })
      })
      .collect(),
  )
}

// A search excerpt is text with the markup's characters escaped as entities, and a no-break space in it is read
// as the space it shows.
fn plain(excerpt: Option<String>) -> Option<String> {
  excerpt.map(|excerpt| decode_entities(&excerpt).replace('\u{a0}', " "))
}

// An ISO time as the minute it names, in UTC.
fn minute(iso: Option<&str>) -> String {
  let Some(iso) = iso else {
    return "unknown".into();
  };

  match chrono::DateTime::parse_from_rfc3339(iso) {
    Ok(time) => format!("{} UTC", time.with_timezone(&chrono::Utc).format("%Y-%m-%d %H:%M")),
    Err(_) => iso.to_owned(),
  }
}

static UPLOADED_IMAGE: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"!\[([^\]|]*)(?:\|[^\]]*)?\]\(upload://[^)]*\)").expect("a fixed pattern"));
static UPLOADED_FILE: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"\[([^\]|]*)(?:\|[^\]]*)?\]\(upload://[^)]*\)").expect("a fixed pattern"));

/// An upload the raw text names by `upload://`, which no reader outside the forum resolves, is named for what it
/// is instead. Only that one link form is rewritten; the rest of the markdown is handed on as its author wrote it.
pub fn readable_markdown(markdown: &str) -> String {
  let images = UPLOADED_IMAGE.replace_all(markdown, |found: &regex::Captures<'_>| {
    let alt = found[1].trim();

    if alt.is_empty() { "[image]".to_owned() } else { format!("[image: {alt}]") }
  });

  UPLOADED_FILE
    .replace_all(&images, |found: &regex::Captures<'_>| format!("[attachment: {}]", found[1].trim()))
    .into_owned()
}

pub fn topic_page(origin: &str, id: i64, slug: &str, post: Option<i64>) -> String {
  match post {
    Some(post) if post > 1 => format!("{origin}/t/{slug}/{id}/{post}"),
    _ => format!("{origin}/t/{slug}/{id}"),
  }
}

fn post_head(post: &RawPost, meta: Option<&ForumPostMeta>, topic: &ForumTopic) -> String {
  let mut parts = vec![
    format!("### #{}", post.number),
    format!(
      "{}{}",
      post.username,
      if meta.is_some_and(|meta| meta.staff) { " (forum staff)" } else { "" }
    ),
    meta.map_or_else(|| format!("{} (last edit)", post.edited), |meta| minute(Some(&meta.created))),
  ];

  if let Some(reply) = meta.and_then(|meta| meta.reply_to) {
    parts.push(format!("reply to #{reply}"));
  }

  if topic.solution.as_ref().is_some_and(|solution| solution.post == post.number) {
    parts.push("marked as the solution".into());
  }

  parts.join(" · ")
}

/// A topic's posts from the one asked for, as many as one answer holds. `more` is the post the next answer
/// starts at, where the topic goes on past these.
pub fn render_topic(
  forum: &DiscourseForum,
  topic: &ForumTopic,
  posts: &[RawPost],
  meta: &BTreeMap<i64, ForumPostMeta>,
  more: Option<i64>,
) -> String {
  let mut blocks = Vec::new();
  let mut used: i64 = 0;
  let mut next = more;

  for (index, post) in posts.iter().enumerate() {
    let head = post_head(post, meta.get(&post.number), topic);
    let body = readable_markdown(&post.text);
    let length = units(&body) as i64;
    let room = POST_BUDGET - used - units(&head) as i64;

    if index > 0 && length > room {
      next = Some(post.number);
      break;
    }

    let shown = if length > room {
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

  let first = posts.first().map(|post| post.number);
  let last = match next {
    None => posts.last().map(|post| post.number),
    Some(next) => posts.iter().rfind(|post| post.number < next).map(|post| post.number),
  };
  let state: Vec<&str> = [topic.closed.then_some("closed"), topic.archived.then_some("archived")]
    .into_iter()
    .flatten()
    .collect();
  let mut lines = vec![
    format!(
      "SOURCE PAGE: {} ({}, read from the forum's own JSON and raw views)",
      topic_page(forum.origin, topic.id, &topic.slug, first),
      forum.forum_name
    ),
    format!("Title: {}", topic.title),
  ];

  if !topic.tags.is_empty() {
    lines.push(format!("Tags: {}", topic.tags.join(", ")));
  }

  lines.push(format!(
    "Started {} · last post {} · {} posts{}{}",
    minute(topic.created.as_deref()),
    minute(topic.last_posted.as_deref()),
    topic.posts,
    topic.views.map_or_else(String::new, |views| format!(" · {views} views")),
    if state.is_empty() { String::new() } else { format!(" · {}", state.join(", ")) }
  ));
  lines.push(match &topic.solution {
    None => "Solution: no reply is marked as the solution.".into(),
    Some(solution) => format!(
      "Solution: #{}{} — {}",
      solution.post,
      if solution.username.is_empty() {
        String::new()
      } else {
        format!(" by {}", solution.username)
      },
      topic_page(forum.origin, topic.id, &topic.slug, Some(solution.post))
    ),
  });
  lines.push(match first {
    None => "No post of this topic stands at that number or after it.".into(),
    Some(first) => format!(
      "POSTS #{first}–#{} (the topic holds {}; a deleted post leaves its number unused), each as its author wrote it in markdown:",
      last.map_or_else(String::new, |last| last.to_string()),
      topic.posts
    ),
  });
  lines.push(String::new());

  for block in blocks {
    lines.push(block);
    lines.push(String::new());
  }

  lines.push(match next {
    None => "End of the topic.".into(),
    Some(next) => format!("MORE: the topic goes on at #{next}; ask again with post {next}."),
  });
  lines.push(
    "Posts are what the forum's members wrote, its staff included; a reply marked as the solution is the one the asker or a moderator picked, not the vendor's documentation."
      .into(),
  );
  lines.join("\n")
}

pub fn render_search(forum: &DiscourseForum, query: &str, hits: &[ForumHit]) -> String {
  let page = format!("{}/search?q={}", forum.origin, url_component(query));

  if hits.is_empty() {
    return format!(
      "SOURCE PAGE: {page} ({} search)\nNo topic of the forum matches {query}.",
      forum.forum_name
    );
  }

  let mut lines = vec![format!(
    "SOURCE PAGE: {page} ({} search, read from the forum's own JSON, at most {SEARCH_LIMIT} topics)",
    forum.forum_name
  )];

  for hit in hits.iter().take(SEARCH_LIMIT) {
    let mut head = vec![
      format!("- {}", hit.title),
      format!("{} posts", hit.posts),
      format!("started {}", minute(hit.created.as_deref())),
      format!("last post {}", minute(hit.last_posted.as_deref())),
    ];

    if hit.solved {
      head.push("solved".into());
    }

    if !hit.tags.is_empty() {
      head.push(format!("tags {}", hit.tags.join(", ")));
    }

    head.push(topic_page(forum.origin, hit.topic, &hit.slug, None));

    let head = head.join(" · ");

    lines.push(match &hit.blurb {
      None => head,
      Some(blurb) => format!("{head}\n  #{}: {blurb}", hit.post.unwrap_or(1)),
    });
  }

  lines.push("Ask again with a topic's address for its posts.".into());
  lines.join("\n")
}

fn unreadable(forum: &DiscourseForum, what: &str) -> String {
  format!(
    "{} UNAVAILABLE: the forum answered nothing readable for {what}.",
    forum.forum_name.to_uppercase()
  )
}

// The raw page holding a post number: the page its number falls on, stepped back while deleted posts have moved
// it earlier, and the pages after it while one answer still has room.
async fn raw_posts(
  forum: &DiscourseForum,
  topic: i64,
  from: i64,
  reach: &dyn Reach,
) -> WebResult<(Vec<RawPost>, Option<i64>)> {
  let mut page = (from.max(1) as usize).div_ceil(RAW_PAGE_SIZE);
  let mut posts = parse_raw(&reach.page(&raw_url(forum.origin, topic, page), None).await?);
  let mut full = posts.len() >= RAW_PAGE_SIZE;
  let mut back = page;

  for _ in 0..STEPS_BACK {
    if back <= 1 || posts.first().map_or(0, |post| post.number) <= from {
      break;
    }

    back -= 1;

    let earlier = parse_raw(&reach.page(&raw_url(forum.origin, topic, back), None).await?);

    posts = earlier.into_iter().chain(posts).collect();
  }

  let mut wanted: Vec<RawPost> = posts.into_iter().filter(|post| post.number >= from).collect();

  for _ in 0..PAGES_ON {
    if !full || wanted.iter().map(|post| units(&post.text) as i64).sum::<i64>() >= POST_BUDGET {
      break;
    }

    page += 1;

    let further = parse_raw(&reach.page(&raw_url(forum.origin, topic, page), None).await?);

    full = further.len() >= RAW_PAGE_SIZE;
    wanted.extend(further);
  }

  let more = if full { wanted.last().map(|post| post.number + 1) } else { None };

  Ok((wanted, more))
}

pub async fn read_topic(
  forum: &DiscourseForum,
  topic: i64,
  from: i64,
  reach: &dyn Reach,
) -> WebResult<String> {
  let body = match reach.page(&topic_json_url(forum.origin, topic), None).await {
    Ok(body) => body,
    Err(error) if matches!(status_of(&error), Some(403 | 404)) => {
      return Ok(format!(
        "SOURCE PAGE: {}/t/{topic} ({})\n{} shows no public topic {topic}: it was deleted, or only the forum's members may read it.",
        forum.origin, forum.forum_name, forum.forum_name
      ));
    }
    Err(error) => return Err(error),
  };
  let Some(read) = parse_topic(&body) else {
    return Ok(unreadable(forum, &format!("topic {topic}")));
  };
  let (posts, more) = raw_posts(forum, topic, from, reach).await?;
  let later = if from > 1 {
    reach
      .page(&posts_json_url(forum.origin, topic, from - 1), None)
      .await
      .unwrap_or_default()
  } else {
    String::new()
  };
  let mut meta = parse_post_meta(&body);

  meta.extend(parse_post_meta(&later));
  Ok(render_topic(forum, &read, &posts, &meta, more))
}

pub async fn read_search(forum: &DiscourseForum, query: &str, reach: &dyn Reach) -> WebResult<String> {
  let body = reach.page(&search_json_url(forum.origin, query), None).await?;

  Ok(match parse_search(&body) {
    None => unreadable(forum, &format!("the search {query}")),
    Some(hits) => render_search(forum, query, &hits),
  })
}

#[derive(Deserialize, JsonSchema)]
pub struct ForumInput {
  /// A topic's address or number, or words to search the forum for.
  query: NonEmpty,
  /// The post number to read from, where an answer said the topic goes on (default: the post the address names, else 1).
  #[schemars(range(min = 1))]
  post: Option<u32>,
}

/// A forum tool's arguments for an address it claims: the search's words, or the address itself.
pub fn forum_claim(forum: &DiscourseForum, url: &Url) -> Option<Value> {
  Some(match forum_request_of(url, forum.origin)? {
    ForumRequest::Search { query } => json!({ "query": query }),
    ForumRequest::Topic { .. } => json!({ "query": url.as_str() }),
  })
}

pub async fn run_forum(forum: &DiscourseForum, input: ForumInput, reach: &dyn Reach) -> WebResult<String> {
  let given = input.query.trim();
  let mut request = match positive(Some(given)) {
    None => ForumRequest::Search {
      query: given.to_owned(),
    },
    Some(topic) => ForumRequest::Topic { topic, post: 1 },
  };
  let is_address = given.get(..7).is_some_and(|head| head.eq_ignore_ascii_case("http://"))
    || given.get(..8).is_some_and(|head| head.eq_ignore_ascii_case("https://"));

  if is_address && let Ok(url) = Url::parse(given) {
    let Some(addressed) = forum_request_of(&url, forum.origin) else {
      return Err(WebError::new(
        format!("{given} is not a {} topic or search page ({})", forum.forum_name, host_of(forum.origin)),
        Failure::Transport,
      ));
    };

    request = addressed;
  }

  match request {
    ForumRequest::Search { query } => read_search(forum, &query, reach).await,
    ForumRequest::Topic { topic, post } => {
      read_topic(forum, topic, input.post.map_or(post, i64::from), reach).await
    }
  }
}
