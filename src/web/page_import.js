// The reading a page takes of itself when it is imported whole. This function runs inside the page, in the
// browser that rendered it, and nowhere else: it is handed to the page as its own source text, so it closes
// over nothing, imports nothing and names no helper outside its body.
//
// It walks the page's main text in document order and writes it as the search result's own markdown and
// structure lines: the title apart, the page's top sections as SECTION lines, the headings under them as
// markdown headings, the font formatting the page's computed styles set (bold, italic, monospace), tables as
// markdown tables, fenced code, and every framed box — a block the page draws with a border or a background
// of its own — as a quotation. Pictures stand where they stood: an image by its own address, and an inline
// SVG or a canvas as a figure the caller captures from the rendered page. A picture never sits inside a
// quotation, since a picture line there is not read as one; the quotation is closed around it and opened
// again. `docs/api.md` 「The explorer entries」 (`explorer/import_page`) is what it answers to.
//
// It answers { url, title, description, keywords, body, figures, references }: a figure is
// { n, kind: "svg" | "canvas", rect: { x, y, width, height }, svg, caption }, its number counted from 1, its
// address the page's own with #figure-<n>, and where it stands in CSS pixels from the document's top left.
function readImportedPage() {
  const figures = [];
  const references = [];
  const pageUrl = location.href.split("#")[0] ?? location.href;
  const summaryHeading = /^(요약|세 ?줄 ?요약|핵심 ?요약|summary|tl;? ?dr|abstract)$/i;
  const referenceHeading = /^(출처|참고 ?문헌|참고 ?자료|참고|references?|sources?|bibliography|further reading)$/i;
  const skipped = new Set(["SCRIPT", "STYLE", "NOSCRIPT", "TEMPLATE", "NAV", "BUTTON", "INPUT", "SELECT", "TEXTAREA", "IFRAME", "OBJECT", "LINK", "META", "HEAD"]);
  const monospace = /mono|consolas|menlo|courier|monaco/i;

  const style = (element) => getComputedStyle(element);

  const hidden = (element) => {
    const computed = style(element);

    return computed.display === "none" || computed.visibility === "hidden" || element.getAttribute("aria-hidden") === "true";
  };

  const isBlock = (element) => {
    const display = style(element).display;

    return !(display === "inline" || display === "inline-block" || display === "contents" || display === "inline-flex");
  };

  // Whether the element holds nothing but inline content: text, formatting and nothing laid out as a block,
  // a picture or a table.
  const inlineOnly = (element) => {
    for (const child of Array.from(element.children)) {
      if (hidden(child)) continue;
      if (["IMG", "SVG", "svg", "CANVAS", "TABLE", "PRE", "UL", "OL", "BLOCKQUOTE", "HR", "FIGURE", "VIDEO", "PICTURE"].includes(child.tagName)) return false;
      if (/^H[1-6]$/.test(child.tagName)) return false;
      if (isBlock(child) || !inlineOnly(child)) return false;
    }

    return true;
  };

  const background = (element) => {
    const color = style(element).backgroundColor;

    return color === "transparent" || /rgba\(\s*\d+,\s*\d+,\s*\d+,\s*0\s*\)/.test(color) ? "" : color;
  };

  const framed = (element) => {
    const computed = style(element);
    const sides = ["Top", "Right", "Bottom", "Left"];
    const bordered = sides.some((side) => {
      const width = Number.parseFloat(computed.getPropertyValue(`border-${side.toLowerCase()}-width`));

      return width > 0 && computed.getPropertyValue(`border-${side.toLowerCase()}-style`) !== "none";
    });
    const parent = element.parentElement;
    const shaded = background(element) !== "" && (parent === null || background(element) !== background(parent));

    return bordered || shaded;
  };

  const absolute = (href) => {
    try {
      return new URL(href, location.href).href;
    } catch {
      return href;
    }
  };

  const inPage = (href) => href.startsWith("#") || absolute(href).split("#")[0] === pageUrl;

  // Delimiters around a run, with the run's own outer spaces moved outside them so the markdown still reads
  // as formatting.
  const wrap = (text, mark) => {
    const inner = text.trim();

    if (inner === "") return text;

    const lead = /^\s/.test(text) ? " " : "";
    const trail = /\s$/.test(text) ? " " : "";

    return `${lead}${mark}${inner}${mark}${trail}`;
  };

  const code = (text) => {
    const inner = text.replace(/\s+/g, " ").trim();

    if (inner === "") return text;

    const longest = Math.max(0, ...(inner.match(/`+/g) ?? []).map((run) => run.length));
    const fence = "`".repeat(longest + 1);
    const pad = inner.startsWith("`") || inner.endsWith("`") ? " " : "";
    const lead = /^\s/.test(text) ? " " : "";
    const trail = /\s$/.test(text) ? " " : "";

    return `${lead}${fence}${pad}${inner}${pad}${fence}${trail}`;
  };

  // What markdown would read as markup in the page's own words, escaped. An underscore inside a word reads
  // as nothing, so only one standing at a word's edge is.
  const escapeText = (text) =>
    text.replace(/([\\`*[\]<>])/g, "\\$1").replace(/(^|[^\p{L}\p{N}])_|_(?=$|[^\p{L}\p{N}])/gu, (found) => found.replace("_", "\\_"));

  // The inline markdown of an element's content. Formatting is read off the computed style rather than the
  // tag, so a span a stylesheet sets in bold or in a monospace face reads as it shows.
  const inline = (node, parentMono, parentBold, parentItalic) => {
    if (node.nodeType === Node.TEXT_NODE) {
      const text = (node.textContent ?? "").replace(/\s+/g, " ");

      return parentMono ? text : escapeText(text);
    }

    if (!(node instanceof Element) || skipped.has(node.tagName) || hidden(node)) return "";
    if (node.tagName === "BR") return "\n";
    if (["IMG", "svg", "SVG", "CANVAS"].includes(node.tagName)) return "";

    const computed = style(node);
    const mono = monospace.test(computed.fontFamily) && !parentMono;
    const bold = (Number.parseInt(computed.fontWeight, 10) >= 600 || computed.fontWeight === "bold") && !parentBold;
    const italic = computed.fontStyle === "italic" && !parentItalic;
    const inner = Array.from(node.childNodes)
      .map((child) => inline(child, parentMono || mono, parentBold || bold, parentItalic || italic))
      .join("");

    if (mono) return code(inner);

    let text = inner;

    if (italic) text = wrap(text, "*");
    if (bold) text = wrap(text, "**");
    if (node.tagName === "A") {
      const href = node.getAttribute("href") ?? "";

      if (href !== "" && !inPage(href) && !href.startsWith("javascript:") && text.trim() !== "") {
        return `[${text.trim()}](<${absolute(href)}>)`;
      }
    }

    return text;
  };

  const inlineOf = (element) => {
    const computed = style(element);
    const mono = monospace.test(computed.fontFamily);
    const bold = Number.parseInt(computed.fontWeight, 10) >= 600;
    const italic = computed.fontStyle === "italic";
    const text = Array.from(element.childNodes)
      .map((child) => inline(child, mono, bold, italic))
      .join("")
      .replace(/[ \t]+\n/g, "\n")
      .replace(/\n[ \t]+/g, "\n")
      .replace(/[ \t]{2,}/g, " ")
      .trim();

    if (text === "") return "";
    // A block the page sets in bold or in a monospace face whole is that formatting whole.
    if (mono) return text.split("\n").map((line) => code(line)).join("  \n");
    if (bold) return wrap(text, "**");

    return text.replace(/\n/g, "  \n");
  };

  // A paragraph whose own words open like one of the result's structure lines is kept from being read as
  // one: the colon is escaped, which markdown shows as the colon it is.
  const structureWord = /^([ \t]*)(ANSWERED|TITLE|TOPIC|KEYWORDS|RULE|KIND|DOCUMENT|VERDICT|SUMMARY|SECTION|INTRO|BODY|CONCLUSION|UNCONFIRMED|MORE|NAME|ADDRESS|MAP|ROADVIEW|RELEASED|NEWER|STOCK|DEAL|IMAGE|REVIEW|SOURCE)([ \t]*(?:\[[^\]]*\])?[ \t]*):/;
  const paragraph = (text) =>
    text
      .split("\n")
      .map((line) => (/^[ \t]*===ITEM===[ \t]*$/.test(line) ? line.replace("===", "\\===") : line.replace(structureWord, "$1$2$3\\:")))
      .join("\n");

  const plain = (element) => (element.textContent ?? "").replace(/\s+/g, " ").trim();

  const figureCaption = (element, n) => {
    const figure = element.closest("figure");
    const caption = figure?.querySelector("figcaption");
    const named = element.getAttribute("aria-label") ?? element.querySelector(":scope > title")?.textContent ?? "";
    const text = plain(caption ?? document.createElement("span")) || named.trim();

    return text === "" ? `Figure ${n}` : text;
  };

  const picture = (element) => {
    const rect = element.getBoundingClientRect();

    if (element.tagName === "IMG") {
      const src = element.currentSrc || element.src;

      if (src === "" || src.startsWith("data:") || (rect.width < 32 && rect.height < 32)) return [];

      const alt = (element.alt || element.title || "").replace(/\s+/g, " ").trim();
      const caption = plain(element.closest("figure")?.querySelector("figcaption") ?? document.createElement("span")) || alt || "Figure";

      return [`IMAGE: ${src} :: ${caption.replace(/::/g, ":")}`];
    }

    if (rect.width < 24 || rect.height < 24) return [];

    const n = figures.length + 1;

    figures.push({
      n,
      kind: element.tagName === "CANVAS" ? "canvas" : "svg",
      rect: { x: rect.left + scrollX, y: rect.top + scrollY, width: rect.width, height: rect.height },
      svg: element.tagName === "CANVAS" ? null : element.outerHTML,
      caption: figureCaption(element, n),
    });

    return [`IMAGE: ${pageUrl}#figure-${n} :: ${figures[figures.length - 1].caption.replace(/::/g, ":")}`];
  };

  // A cell on one line. A header cell is bold by the table's own nature, so its bold is not written again.
  const cell = (element) => {
    const text = inlineOf(element).replace(/\s*\n\s*/g, " ").replace(/\|/g, "\\|");

    return element.tagName === "TH" ? text.replace(/^\*\*(.*)\*\*$/, "$1") : text;
  };

  const table = (element) => {
    const rows = Array.from(element.rows).filter((row) => !hidden(row));

    if (rows.length === 0) return [];

    const grid = rows.map((row) => Array.from(row.cells).flatMap((one) => Array.from({ length: Math.max(1, one.colSpan) }, (_, at) => (at === 0 ? cell(one) : ""))));
    const width = Math.max(...grid.map((row) => row.length));
    const padded = grid.map((row) => [...row, ...Array.from({ length: width - row.length }, () => "")]);
    const headed = rows[0].parentElement?.tagName === "THEAD" || Array.from(rows[0].cells).every((one) => one.tagName === "TH");
    const head = headed ? padded[0] : Array.from({ length: width }, () => "");
    const body = headed ? padded.slice(1) : padded;
    const line = (row) => `| ${row.join(" | ")} |`;
    const caption = element.caption === null ? [] : [`**${plain(element.caption)}**`, ""];

    return [...caption, line(head), line(head.map(() => "---")), ...body.map(line)];
  };

  const list = (element, depth) => {
    const ordered = element.tagName === "OL";
    const start = ordered ? Number.parseInt(element.getAttribute("start") ?? "1", 10) || 1 : 1;
    const lines = [];
    let index = start;

    for (const item of Array.from(element.children)) {
      if (item.tagName !== "LI" || hidden(item)) continue;

      const marker = ordered ? `${index}. ` : "- ";
      const indent = "   ".repeat(depth);
      const nested = Array.from(item.children).filter((child) => child.tagName === "UL" || child.tagName === "OL");

      // The item's own text is read off the live item less its lists, since only the live item is laid out.
      const text = Array.from(item.childNodes)
        .filter((child) => !(child instanceof Element && (child.tagName === "UL" || child.tagName === "OL")))
        .map((child) => inline(child, false, false, false))
        .join("")
        .replace(/\s+/g, " ")
        .trim();

      lines.push(`${indent}${marker}${text}`);

      for (const sub of nested) lines.push(...list(sub, depth + 1));

      index += 1;
    }

    return lines;
  };

  // A box's title: a short run set in bold standing first in it, alone on its line.
  const boxTitle = (element) => {
    const first = Array.from(element.children).find((child) => !hidden(child));

    if (first === undefined || element.firstChild === null) return null;

    const before = Array.from(element.childNodes).slice(0, Array.from(element.childNodes).indexOf(first));

    if (before.some((node) => (node.textContent ?? "").trim() !== "")) return null;

    const weight = Number.parseInt(style(first).fontWeight, 10);

    return isBlock(first) && weight >= 600 && plain(first).length <= 80 ? first : null;
  };

  // A list of links into the page itself: the page's own table of contents.
  const contents = (element) => {
    const links = Array.from(element.querySelectorAll("a[href]"));
    const inner = links.filter((link) => inPage(link.getAttribute("href") ?? ""));
    const linked = inner.reduce((sum, link) => sum + plain(link).length, 0);

    return element.querySelector("ol, ul") !== null && inner.length >= 3 && inner.length / links.length >= 0.6 && linked / Math.max(1, plain(element).length) >= 0.6;
  };

  let titleElement = null;
  let sectionLevel = 2;

  const heading = (element) => {
    const level = Number.parseInt(element.tagName.slice(1), 10);
    const text = plain(element);

    if (text === "" || element === titleElement) return [];
    if (level <= sectionLevel) {
      if (summaryHeading.test(text)) return ["SUMMARY:"];

      return [`SECTION: ${text}`];
    }

    return [`${"#".repeat(Math.min(6, level - sectionLevel))} ${text}`];
  };

  // The blocks of an element in order. `quoted` is true inside a framed box, where a heading reads as a bold
  // line and a picture closes the quotation around it.
  const blocks = (element, quoted) => {
    const out = [];
    let run = [];

    const flush = () => {
      if (run.length === 0) return;

      const text = run
        .map((node) => inline(node, monospace.test(style(element).fontFamily), Number.parseInt(style(element).fontWeight, 10) >= 600, style(element).fontStyle === "italic"))
        .join("")
        .replace(/[ \t]+\n/g, "\n")
        .replace(/\n[ \t]+/g, "\n")
        .replace(/[ \t]{2,}/g, " ")
        .trim();

      run = [];

      if (text !== "") out.push([paragraph(text.replace(/\n/g, "  \n"))]);
    };

    for (const node of Array.from(element.childNodes)) {
      if (node.nodeType === Node.TEXT_NODE) {
        run.push(node);
        continue;
      }

      if (!(node instanceof Element) || skipped.has(node.tagName) || hidden(node)) continue;

      const tag = node.tagName;

      if (tag === "svg" || tag === "SVG" || tag === "CANVAS" || tag === "IMG") {
        flush();

        const lines = picture(node);

        if (lines.length > 0) out.push(lines);

        continue;
      }

      if (!isBlock(node) && tag !== "PICTURE" && inlineOnly(node) && node.querySelector("img, svg, canvas") === null) {
        run.push(node);
        continue;
      }

      flush();
      out.push(...block(node, quoted));
    }

    flush();

    return out;
  };

  const block = (element, quoted) => {
    const tag = element.tagName;

    if (/^H[1-6]$/.test(tag)) {
      if (quoted) return [[`**${plain(element)}**`]];

      const lines = heading(element);

      return lines.length === 0 ? [] : [lines];
    }

    if (tag === "P") {
      const pictures = element.querySelector("img, svg, canvas");

      if (pictures === null) {
        const text = inlineOf(element);

        return text === "" ? [] : [[paragraph(text)]];
      }

      return blocks(element, quoted);
    }

    if (tag === "HR") return quoted ? [] : [["---"]];
    if (tag === "PRE") {
      const text = (element.textContent ?? "").replace(/\n$/, "");
      const language = /(?:language|lang)-([\w+-]+)/.exec(`${element.className} ${element.querySelector("code")?.className ?? ""}`)?.[1] ?? "";
      const fence = /```/.test(text) ? "~~~~" : "```";

      return [[`${fence}${language}`, ...text.split("\n"), fence]];
    }
    if (tag === "TABLE") return [table(element)];
    if (tag === "UL" || tag === "OL") return [list(element, 0)];
    if (tag === "FIGCAPTION") return [];
    if (tag === "BLOCKQUOTE") return quote(element);
    if (tag === "DETAILS") {
      const summary = element.querySelector(":scope > summary");
      const was = element.open;

      element.open = true;

      const inner = blocks(element, quoted).filter((lines) => lines.join("\n") !== inlineOf(summary ?? document.createElement("span")));

      element.open = was;

      return [[`**${plain(summary ?? document.createElement("span"))}**`], ...inner];
    }

    if (!quoted && contents(element)) {
      const title = boxTitle(element);
      const lists = Array.from(element.querySelectorAll(":scope ol, :scope ul")).filter((one) => one.parentElement?.closest("ol, ul") === null);

      return [[`SECTION: ${title === null ? "Contents" : plain(title)}`], ...lists.map((one) => list(one, 0))];
    }

    // A row laid out side by side whose parts are each a run of words — a numbered step, a label and its
    // value — reads as one line, with a leading number standing as the list's own.
    const display = style(element).display;

    if ((display === "flex" || display === "grid") && Array.from(element.children).every((child) => inlineOnly(child)) && element.querySelector("img, svg, canvas, table, pre") === null) {
      const shown = Array.from(element.children).filter((child) => !hidden(child) && inlineOf(child) !== "");
      const parts = shown.map((child) => inlineOf(child));
      const first = shown[0] === undefined ? "" : plain(shown[0]);

      if (parts.length > 1 && /^\d{1,3}$/.test(first)) return [[`${first}. ${paragraph(parts.slice(1).join(" "))}`]];

      return parts.length === 0 ? [] : [[paragraph(parts.join(" "))]];
    }

    if (!quoted && framed(element) && element !== document.body) return quote(element);

    if (inlineOnly(element) && element.querySelector("img, svg, canvas") === null) {
      const text = inlineOf(element);

      return text === "" ? [] : [[paragraph(text)]];
    }

    return blocks(element, quoted);
  };

  // A framed box as a quotation: its title in bold on the first line, and a picture inside it set between
  // two quotations rather than inside one.
  const quote = (element) => {
    const title = boxTitle(element);
    const parts = [];

    if (title !== null) parts.push([`**${plain(title)}**`]);

    for (const lines of blocks(element, true)) {
      if (title !== null && lines.length === 1 && lines[0] === inlineOf(title)) continue;

      parts.push(lines);
    }

    const out = [];
    let open = [];

    const close = () => {
      if (open.length > 0) out.push(open);
      open = [];
    };

    for (const lines of parts) {
      if (lines.some((line) => line.startsWith("IMAGE: "))) {
        close();
        out.push(lines);
        continue;
      }

      if (open.length > 0) open.push(">");

      open.push(...lines.flatMap((line) => line.split("\n")).map((line) => (line === "" ? ">" : `> ${line}`)));
    }

    close();

    return out;
  };

  const candidates = Array.from(document.querySelectorAll("article, main, [role=main]")).filter((element) => !hidden(element));
  const root = candidates.sort((a, b) => plain(b).length - plain(a).length)[0] ?? document.body;
  const headings = Array.from(root.querySelectorAll("h1, h2, h3, h4, h5, h6")).filter((element) => !hidden(element));
  const firstH1 = headings.find((element) => element.tagName === "H1") ?? null;

  titleElement = firstH1 !== null && headings.filter((element) => element.tagName === "H1").length === 1 ? firstH1 : null;
  sectionLevel = Math.min(6, ...headings.filter((element) => element !== titleElement).map((element) => Number.parseInt(element.tagName.slice(1), 10)));

  // The page's own reference list, where a section of it is named as one: its links are the pages the page
  // itself cites, and they stand among the import's sources.
  for (const element of headings) {
    if (!referenceHeading.test(plain(element))) continue;

    let next = element.nextElementSibling;

    while (next !== null && !/^H[1-6]$/.test(next.tagName)) {
      for (const link of Array.from(next.querySelectorAll("a[href]"))) {
        const href = link.getAttribute("href") ?? "";

        if (!inPage(href) && /^https?:/i.test(absolute(href))) references.push({ text: plain(link), url: absolute(href) });
      }

      next = next.nextElementSibling;
    }
  }

  const body = blocks(root, false)
    .map((lines) => lines.join("\n"))
    .filter((text) => text.trim() !== "")
    .join("\n\n");
  const meta = (name) => document.querySelector(`meta[name="${name}"], meta[property="og:${name}"]`)?.getAttribute("content")?.trim() ?? "";

  return {
    url: location.href,
    title: (titleElement === null ? "" : plain(titleElement)) || meta("title") || document.title.trim(),
    description: meta("description"),
    keywords: meta("keywords").split(",").map((word) => word.trim()).filter((word) => word !== ""),
    body,
    figures,
    references,
  };
}
