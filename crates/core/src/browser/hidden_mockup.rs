//! Product mockups the page itself declares are not content (decision 1 of
//! round 13: skip a mockup only when we are really sure it is one).
//!
//! A drawing of an interface reports as advisory elsewhere (`role="img"`, a
//! framed demo, a mockup class: [`super::decorative_text::box_in_mockup_dom`],
//! [`super::text_context::in_mock_context_dom`]). Those are guesses read off
//! the drawing. This module reads what the author declared instead: a
//! subtree under `inert` (M1) or `aria-hidden="true"` (M2) has been taken
//! out of the page's content on purpose, so the findings that score its
//! text and boxes as content ([`DROP_RULES`]) are dropped there, and its
//! headings leave the outline `skipped-heading` reads.
//!
//! Hiding a subtree is also how a page parks content a visitor will reach
//! (a carousel's resting slides, a marquee's second copy, the page behind
//! an open modal, a closed menu caught mid-transition, a split-text
//! headline whose words are spans), so a hidden root counts only when every
//! gate in [`HiddenMockups::is_root`] holds. Anything the gates cannot
//! settle keeps the base behaviour.
//!
//! A frame-sized box whose `aria-label` names a screenshot or an
//! illustration (`aria-label="A screenshot of the Linear app"`) is a
//! description, not a statement that the subtree is not content: findings
//! under it report as advisory and are never dropped (taste call r13-t5).

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;

use once_cell::sync::Lazy;
use regex::Regex;

use super::dom::{class_attr, tag_lower, Dom, ElId};
use crate::checks::decorative_text::token_parts;
use crate::js;

/// The rules that score a mockup's text and boxes as if they were the
/// page's content. Under a declared mockup they report nothing; under a
/// labelled screenshot they report as advisory.
pub const DROP_RULES: &[&str] = &[
    "low-contrast",
    "nested-cards",
    "tiny-text",
    "undersized-ui-text",
    "cramped-padding",
    "clipped-overflow-container",
];

/// A drawn window is at least this large (the framed-demo size floor).
const FRAME_MIN_WIDTH_PX: f64 = 120.0;
const FRAME_MIN_HEIGHT_PX: f64 = 80.0;
/// A mockup draws an interface: at least this many separate runs of text.
/// An `aria-hidden` icon, badge or decorative copy of one line has fewer.
const MIN_TEXT_RUNS: usize = 3;
/// A subtree larger than this is not a drawing; the walk stops and the
/// root does not count.
const MAX_ROOT_NODES: usize = 3000;

/// Tags a declared mockup root never is: the document's own structure and
/// its landmarks (an `inert` `<main>` or `<header>` sits behind a modal),
/// list items and table parts (a marquee's or carousel's copies), and
/// dialogs.
const ROOT_SKIP_TAGS: &[&str] = &[
    "html", "body", "main", "header", "footer", "nav", "aside", "section", "article", "form",
    "dialog", "li", "tr", "td", "th", "tbody", "thead", "table", "details", "summary",
];
/// Roles a declared mockup root may carry. Any other role names a part of
/// the page's working structure (a tab panel, a menu, a landmark, an
/// option), which a page parks as hidden while it is not the current one.
const ROOT_ROLES: &[&str] = &["presentation", "none", "img", "figure", "group"];
/// Ancestors that make a hidden root part of a line of text, not a drawing:
/// a split-text headline (`<h1><span aria-hidden>` per word), a link's or
/// button's decorative copy.
const TEXT_HOST_TAGS: &[&str] = &[
    "h1", "h2", "h3", "h4", "h5", "h6", "p", "a", "button", "label", "summary", "legend",
    "caption", "figcaption", "blockquote", "q",
];
/// Class or id parts that name a copy, a slide, or a piece of the page's
/// own chrome that is hidden while closed. A root wearing one is content the
/// visitor reaches, not a drawing.
const ROOT_SKIP_PARTS: &[&str] = &[
    "clone", "clones", "cloned", "copy", "duplicate", "dup", "carousel", "embla", "flicking",
    "glide", "keen", "slick", "slide", "slider", "slides", "splide", "swiper", "marquee",
    "ticker", "menu", "submenu", "dropdown", "popup", "popover", "modal", "drawer", "dialog",
    "tooltip", "toast", "offcanvas", "overlay", "nav", "navbar", "navigation", "panel", "tab",
    "tabpanel", "accordion", "collapse", "collapsible", "intro", "splash", "loader", "preloader",
    "loading", "banner", "cookie", "cookies", "consent",
];
/// Elements whose text is source, not words on the page.
const SOURCE_TEXT_TAGS: &[&str] = &["script", "style", "noscript", "template"];

/// An `aria-label` that describes the box as a picture of something.
static PICTURE_LABEL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(?:screen ?shots?|screen ?captures?|screencaps?|illustrations?|mock-?ups?)\b|\bpreview of\b")
        .expect("PICTURE_LABEL_RE")
});

/// What the page declares about an element's text and boxes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MockupSignal {
    /// Under an `inert` or `aria-hidden="true"` mockup: not content.
    Declared,
    /// Under a box labelled as a screenshot or illustration: advisory.
    Labelled,
}

/// One page's declared mockups, judged once per candidate root.
pub struct HiddenMockups<'a> {
    dom: &'a dyn Dom,
    /// Candidates whose attributes make them worth asking about; every
    /// other element answers `None` without a walk.
    hidden: Vec<ElId>,
    labelled: Vec<ElId>,
    roots: RefCell<HashMap<ElId, bool>>,
    labels: RefCell<HashMap<ElId, bool>>,
    page_chars: OnceCell<usize>,
    modal_open: OnceCell<Option<Vec<ElId>>>,
}

impl<'a> HiddenMockups<'a> {
    /// The page's candidates: elements carrying `inert`, `aria-hidden` or
    /// an `aria-label` that names a picture. Nothing is judged yet.
    pub fn scan(dom: &'a dyn Dom) -> Self {
        let mut hidden = Vec::new();
        let mut labelled = Vec::new();
        for el in dom.query_all(None, "*").unwrap_or_default() {
            if declares_hidden(dom, el) {
                hidden.push(el);
            }
            if dom.attr(el, "aria-label").is_some_and(|l| PICTURE_LABEL_RE.is_match(&l)) {
                labelled.push(el);
            }
        }
        HiddenMockups {
            dom,
            hidden,
            labelled,
            roots: RefCell::new(HashMap::new()),
            labels: RefCell::new(HashMap::new()),
            page_chars: OnceCell::new(),
            modal_open: OnceCell::new(),
        }
    }

    /// No candidate on the page: nothing to ask.
    pub fn is_empty(&self) -> bool {
        self.hidden.is_empty() && self.labelled.is_empty()
    }

    /// What the page declares about `el`: under a declared mockup (the
    /// element itself or an ancestor), under a labelled screenshot, or
    /// neither. A declared root anywhere above wins over a label.
    pub fn signal(&self, el: ElId) -> Option<MockupSignal> {
        if self.is_empty() {
            return None;
        }
        let mut labelled = false;
        let mut cur = Some(el);
        while let Some(c) = cur {
            if self.hidden.contains(&c) && self.is_root(c) {
                return Some(MockupSignal::Declared);
            }
            if !labelled && self.labelled.contains(&c) && self.is_labelled_picture(c) {
                labelled = true;
            }
            cur = self.dom.parent(c);
        }
        labelled.then_some(MockupSignal::Labelled)
    }

    /// Whether `el` sits under a declared mockup.
    pub fn declared(&self, el: ElId) -> bool {
        self.signal(el) == Some(MockupSignal::Declared)
    }

    /// Whether a hidden element is a drawn mockup the author took out of
    /// the page's content. Cheap gates first; the page's text is counted
    /// once, and only for a root every other gate passed.
    fn is_root(&self, el: ElId) -> bool {
        if let Some(&known) = self.roots.borrow().get(&el) {
            return known;
        }
        let yes = self.judge_root(el);
        self.roots.borrow_mut().insert(el, yes);
        yes
    }

    fn judge_root(&self, el: ElId) -> bool {
        let dom = self.dom;
        if !frame_sized(dom, el) || !structural_box(dom, el) || under_text_host(dom, el) {
            return false;
        }
        let Some(own) = subtree_text(dom, el) else { return false };
        if own.runs < MIN_TEXT_RUNS || own.h1 {
            return false;
        }
        if has_content_peer(dom, el, &own.text) {
            return false;
        }
        if self.behind_open_modal(el) {
            return false;
        }
        // A drawing holds a small part of the page's words. The page behind
        // an open modal, or a template's whole body, holds most of them.
        let page = *self.page_chars.get_or_init(|| page_text_chars(dom));
        page > 0 && own.chars * 2 < page
    }

    /// Whether a dialog is open on the page outside `el`: the libraries
    /// that open one hide or make inert everything else, which is then the
    /// page, not a drawing. Not every library sets `aria-modal`
    /// (chefsspecial.app's open `role="dialog"` hid a fixed bottom bar), so
    /// any shown dialog counts, a cookie notice's included: a page with one
    /// open keeps the base behaviour. A dialog that is itself hidden or
    /// inert hides nothing.
    fn behind_open_modal(&self, el: ElId) -> bool {
        let dom = self.dom;
        let modals = self.modal_open.get_or_init(|| {
            let open: Vec<ElId> = dom
                .query_all(None, "*")
                .unwrap_or_default()
                .into_iter()
                .filter(|&m| is_dialog(dom, m) && !declares_hidden(dom, m) && shown(dom, m))
                .collect();
            (!open.is_empty()).then_some(open)
        });
        modals
            .as_ref()
            .is_some_and(|open| open.iter().any(|&m| !dom.contains(m, el) && !dom.contains(el, m)))
    }

    fn is_labelled_picture(&self, el: ElId) -> bool {
        if let Some(&known) = self.labels.borrow().get(&el) {
            return known;
        }
        let dom = self.dom;
        let yes = frame_sized(dom, el)
            && structural_box(dom, el)
            && !matches!(tag_lower(dom, el).as_str(), "a" | "button" | "input" | "select" | "textarea");
        self.labels.borrow_mut().insert(el, yes);
        yes
    }
}

/// A dialog: `aria-modal="true"`, `role="dialog"` or `"alertdialog"`, or
/// an open `<dialog>`.
fn is_dialog(dom: &dyn Dom, el: ElId) -> bool {
    dom.attr(el, "aria-modal")
        .is_some_and(|v| js::to_lower_case(js::trim(&v)) == "true")
        || dom.attr(el, "role").is_some_and(|r| {
            r.split_ascii_whitespace()
                .any(|t| matches!(js::to_lower_case(t).as_str(), "dialog" | "alertdialog"))
        })
        || (tag_lower(dom, el) == "dialog" && dom.attr(el, "open").is_some())
}

/// `inert` in any form, or `aria-hidden="true"`.
fn declares_hidden(dom: &dyn Dom, el: ElId) -> bool {
    dom.attr(el, "inert").is_some()
        || dom
            .attr(el, "aria-hidden")
            .is_some_and(|v| js::to_lower_case(js::trim(&v)) == "true")
}

/// At least the framed-demo floor on both axes. A box with no layout
/// (`display: none`, or a static adapter's unknown size) is no evidence.
fn frame_sized(dom: &dyn Dom, el: ElId) -> bool {
    let r = dom.rect(el);
    r.width >= FRAME_MIN_WIDTH_PX && r.height >= FRAME_MIN_HEIGHT_PX
}

/// Painted at capture by its own style: laid out, not hidden.
fn shown(dom: &dyn Dom, el: ElId) -> bool {
    let r = dom.rect(el);
    r.width > 0.0
        && r.height > 0.0
        && dom.style(el, "display") != "none"
        && dom.style(el, "visibility") != "hidden"
}

/// Neither the page's structure nor a working part of it: not a skipped
/// tag, a role outside [`ROOT_ROLES`], a dialog, or a box whose class or id
/// names a copy, a slide or the page's chrome.
fn structural_box(dom: &dyn Dom, el: ElId) -> bool {
    if ROOT_SKIP_TAGS.contains(&tag_lower(dom, el).as_str()) {
        return false;
    }
    if let Some(role) = dom.attr(el, "role") {
        let role = js::to_lower_case(js::trim(&role));
        if !role.is_empty() && !role.split_ascii_whitespace().all(|r| ROOT_ROLES.contains(&r)) {
            return false;
        }
    }
    // `data-aria-hidden` is the mark the modal libraries (`aria-hidden`,
    // Radix, Headless UI) leave on what they hid behind an open dialog.
    if dom.attr(el, "aria-modal").is_some() || dom.attr(el, "data-aria-hidden").is_some() {
        return false;
    }
    // A drawing sits in the flow of the page or is placed over a section.
    // A box fixed or stuck to the viewport is the page's chrome: a bar, an
    // intro splash, a banner.
    if matches!(dom.style(el, "position").as_str(), "fixed" | "sticky") {
        return false;
    }
    let names = format!("{} {}", class_attr(dom, el), dom.attr(el, "id").unwrap_or_default());
    !names.split_whitespace().any(|token| {
        token_parts(token)
            .iter()
            .any(|p| ROOT_SKIP_PARTS.contains(&p.as_str()))
    })
}

/// Whether an ancestor makes the root part of a line of text.
fn under_text_host(dom: &dyn Dom, el: ElId) -> bool {
    let mut cur = dom.parent(el);
    while let Some(c) = cur {
        if TEXT_HOST_TAGS.contains(&tag_lower(dom, c).as_str()) {
            return true;
        }
        cur = dom.parent(c);
    }
    false
}

/// What a candidate root holds.
struct SubtreeText {
    /// Elements with words of their own.
    runs: usize,
    /// Non-whitespace characters.
    chars: usize,
    /// Its characters with every space removed, for comparison with a
    /// peer (split text joins its words with or without spaces).
    text: String,
    h1: bool,
}

/// The root's runs of text, characters and words, or `None` when the
/// subtree is too large to be a drawing.
fn subtree_text(dom: &dyn Dom, el: ElId) -> Option<SubtreeText> {
    let mut out = SubtreeText { runs: 0, chars: 0, text: String::new(), h1: false };
    let mut stack = vec![el];
    let mut visited = 0usize;
    while let Some(n) = stack.pop() {
        visited += 1;
        if visited > MAX_ROOT_NODES {
            return None;
        }
        let tag = tag_lower(dom, n);
        if SOURCE_TEXT_TAGS.contains(&tag.as_str()) {
            continue;
        }
        if tag == "h1" {
            out.h1 = true;
        }
        let direct = dom.direct_text_nodes(n).join(" ");
        let chars = direct.chars().filter(|c| !c.is_whitespace()).count();
        if chars > 0 {
            out.runs += 1;
            out.chars += chars;
        }
        stack.extend(dom.children(n).into_iter().rev());
    }
    out.text = squash(&dom.text_content(el));
    Some(out)
}

fn squash(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether the root is one copy of content the page also shows: a sibling
/// of the same tag and class that is shown and not itself hidden (a
/// marquee's first copy, a carousel's current slide), a sibling or the
/// parent's label carrying the same words (a visually hidden text copy of
/// a split or animated one).
fn has_content_peer(dom: &dyn Dom, el: ElId, text: &str) -> bool {
    let tag = tag_lower(dom, el);
    let class = collapse(&class_attr(dom, el));
    if let Some(parent) = dom.parent(el) {
        if dom
            .attr(parent, "aria-label")
            .is_some_and(|l| !text.is_empty() && squash(&l) == text)
        {
            return true;
        }
        for sib in dom.children(parent) {
            if sib == el {
                continue;
            }
            let same_shape = tag_lower(dom, sib) == tag && collapse(&class_attr(dom, sib)) == class;
            if same_shape && shown(dom, sib) && !declares_hidden(dom, sib) {
                return true;
            }
            if !text.is_empty() && squash(&dom.text_content(sib)) == text {
                return true;
            }
        }
    }
    false
}

/// The page's rendered words, counted once.
fn page_text_chars(dom: &dyn Dom) -> usize {
    let Some(root) = dom.body().or_else(|| dom.document_element()) else { return 0 };
    let mut count = 0usize;
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        if SOURCE_TEXT_TAGS.contains(&tag_lower(dom, n).as_str()) {
            continue;
        }
        count += dom
            .direct_text_nodes(n)
            .iter()
            .map(|t| t.chars().filter(|c| !c.is_whitespace()).count())
            .sum::<usize>();
        stack.extend(dom.children(n));
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::fake_dom::FakeDom;

    /// A page with some copy of its own and an `h1`, so a small drawing
    /// holds well under half of its words.
    fn page() -> (FakeDom, ElId) {
        let mut d = FakeDom::new();
        let (_html, body) = d.with_page();
        d.set_rect(body, 0.0, 0.0, 1280.0, 4000.0);
        let h1 = d.add(Some(body), "h1");
        d.set_rect(h1, 0.0, 0.0, 800.0, 60.0);
        d.add_text(h1, "Ship the product your team has been waiting for");
        let p = d.add(Some(body), "p");
        d.set_rect(p, 0.0, 80.0, 800.0, 200.0);
        d.add_text(
            p,
            "A long paragraph of the page's own copy that tells the visitor what the product does, \
             who it is for, and why it is better than what they use today, with enough words to \
             outweigh any drawing on the page by a wide margin.",
        );
        (d, body)
    }

    /// A drawn window: a frame-sized box holding `labels`, one run each.
    fn window(d: &mut FakeDom, parent: ElId, labels: &[&str]) -> (ElId, Vec<ElId>) {
        let w = d.add(Some(parent), "div");
        d.set_rect(w, 100.0, 400.0, 600.0, 400.0);
        let mut runs = Vec::new();
        for (i, l) in labels.iter().enumerate() {
            let s = d.add(Some(w), "span");
            d.set_rect(s, 110.0, 410.0 + 20.0 * i as f64, 100.0, 14.0);
            d.add_text(s, l);
            runs.push(s);
        }
        (w, runs)
    }

    const LABELS: &[&str] = &["Inbox", "Drafts", "Sent", "Archive"];

    #[test]
    fn an_aria_hidden_window_is_a_declared_mockup() {
        let (mut d, body) = page();
        let (w, runs) = window(&mut d, body, LABELS);
        d.set_attr(w, "aria-hidden", "true");
        let m = HiddenMockups::scan(&d);
        assert_eq!(m.signal(runs[0]), Some(MockupSignal::Declared));
        assert!(m.declared(w), "the root itself");
    }

    #[test]
    fn an_inert_window_is_a_declared_mockup() {
        let (mut d, body) = page();
        let (w, runs) = window(&mut d, body, LABELS);
        d.set_attr(w, "inert", "");
        assert!(HiddenMockups::scan(&d).declared(runs[1]));
    }

    #[test]
    fn nothing_hidden_is_no_mockup() {
        // A live demo with nothing hidden keeps its findings.
        let (mut d, body) = page();
        let (_w, runs) = window(&mut d, body, LABELS);
        assert_eq!(HiddenMockups::scan(&d).signal(runs[0]), None);
    }

    #[test]
    fn aria_hidden_false_is_no_mockup() {
        let (mut d, body) = page();
        let (w, runs) = window(&mut d, body, LABELS);
        d.set_attr(w, "aria-hidden", "false");
        assert_eq!(HiddenMockups::scan(&d).signal(runs[0]), None);
    }

    #[test]
    fn a_hidden_badge_is_too_small() {
        let (mut d, body) = page();
        let (w, runs) = window(&mut d, body, LABELS);
        d.set_attr(w, "aria-hidden", "true");
        d.set_rect(w, 100.0, 400.0, 110.0, 60.0);
        assert!(!HiddenMockups::scan(&d).declared(runs[0]));
    }

    #[test]
    fn a_hidden_decorative_heading_copy_has_too_few_runs() {
        let (mut d, body) = page();
        let (w, runs) = window(&mut d, body, &["Build", "faster"]);
        d.set_attr(w, "aria-hidden", "true");
        assert!(!HiddenMockups::scan(&d).declared(runs[0]));
    }

    #[test]
    fn an_app_root_hidden_behind_a_modal_is_the_page() {
        // chefsspecial.app: the whole app aria-hidden behind an open modal,
        // its h1 inside, most of the page's words.
        let mut d = FakeDom::new();
        let (_html, body) = d.with_page();
        d.set_rect(body, 0.0, 0.0, 1280.0, 4000.0);
        let app = d.add(Some(body), "div");
        d.set_rect(app, 0.0, 0.0, 1280.0, 4000.0);
        d.set_attr(app, "aria-hidden", "true");
        let h1 = d.add(Some(app), "h1");
        d.set_rect(h1, 0.0, 0.0, 800.0, 60.0);
        d.add_text(h1, "Recipes");
        let mut runs = Vec::new();
        for i in 0..5 {
            let s = d.add(Some(app), "p");
            d.set_rect(s, 0.0, 100.0 + 40.0 * i as f64, 800.0, 30.0);
            d.add_text(s, "A recipe card with its title and a short description");
            runs.push(s);
        }
        let modal = d.add(Some(body), "div");
        d.set_rect(modal, 300.0, 200.0, 600.0, 400.0);
        d.set_attr(modal, "role", "dialog");
        d.set_attr(modal, "aria-modal", "true");
        assert!(!HiddenMockups::scan(&d).declared(runs[0]), "the h1 is inside");
        // Without its h1 the app still holds most of the page's words.
        d.el_mut(h1).tag = "div".into();
        d.set_attr(modal, "aria-modal", "false");
        assert!(!HiddenMockups::scan(&d).declared(runs[0]), "most of the page");
    }

    #[test]
    fn a_wrapper_hidden_behind_an_open_modal_is_not_a_mockup() {
        let (mut d, body) = page();
        let (w, runs) = window(&mut d, body, LABELS);
        d.set_attr(w, "aria-hidden", "true");
        let modal = d.add(Some(body), "div");
        d.set_rect(modal, 300.0, 200.0, 600.0, 400.0);
        d.set_attr(modal, "aria-modal", "true");
        assert!(!HiddenMockups::scan(&d).declared(runs[0]));
        // A dialog with no aria-modal hides the page as well
        // (chefsspecial.app's Radix dialog).
        d.set_attr(modal, "aria-modal", "false");
        d.set_attr(modal, "role", "dialog");
        assert!(!HiddenMockups::scan(&d).declared(runs[0]), "role=dialog");
        // A modal that is not shown says nothing.
        d.set_style(modal, "display", "none");
        assert!(HiddenMockups::scan(&d).declared(runs[0]));
    }

    #[test]
    fn a_hidden_dialog_or_drawer_on_screen_is_not_a_mockup() {
        let (mut d, body) = page();
        let (w, runs) = window(&mut d, body, LABELS);
        d.set_attr(w, "inert", "");
        d.set_attr(w, "role", "dialog");
        assert!(!HiddenMockups::scan(&d).declared(runs[0]), "a dialog");
        let (mut d, body) = page();
        let (w, runs) = window(&mut d, body, LABELS);
        d.set_attr(w, "inert", "");
        d.set_attr(w, "class", "site-drawer is-closing");
        assert!(!HiddenMockups::scan(&d).declared(runs[0]), "a drawer");
        let (mut d, body) = page();
        let w = d.add(Some(body), "dialog");
        d.set_rect(w, 100.0, 400.0, 600.0, 400.0);
        d.set_attr(w, "inert", "");
        let mut last = w;
        for l in LABELS {
            last = d.add(Some(w), "span");
            d.set_rect(last, 110.0, 410.0, 100.0, 14.0);
            d.add_text(last, l);
        }
        assert!(!HiddenMockups::scan(&d).declared(last), "a <dialog>");
    }

    #[test]
    fn a_marquee_copy_is_not_a_mockup() {
        // framer.com 10133514: an aria-hidden `li.clone-item` copy of
        // visible cards.
        let (mut d, body) = page();
        let track = d.add(Some(body), "ul");
        d.set_rect(track, 0.0, 400.0, 2400.0, 400.0);
        let (first, _) = window(&mut d, track, LABELS);
        let (copy, runs) = window(&mut d, track, LABELS);
        d.el_mut(first).tag = "li".into();
        d.el_mut(copy).tag = "li".into();
        d.set_attr(first, "class", "clone-item");
        d.set_attr(copy, "class", "clone-item");
        d.set_attr(copy, "aria-hidden", "true");
        assert!(!HiddenMockups::scan(&d).declared(runs[0]), "li.clone-item");
        // A div copy of the same shape beside a shown original.
        d.el_mut(first).tag = "div".into();
        d.el_mut(copy).tag = "div".into();
        d.set_attr(first, "class", "logo-set");
        d.set_attr(copy, "class", "logo-set");
        assert!(!HiddenMockups::scan(&d).declared(runs[0]), "a shown peer of the same shape");
        // Even hidden, a peer carrying the same words is a copy.
        d.set_attr(first, "aria-hidden", "true");
        assert!(!HiddenMockups::scan(&d).declared(runs[0]), "a hidden copy of the same words");
        // Two hidden windows of one shape drawing different things
        // (cursor.com's stacked demo windows) are both drawings.
        let (mut d, body) = page();
        let stage = d.add(Some(body), "div");
        d.set_rect(stage, 0.0, 400.0, 1280.0, 700.0);
        let (ide, _) = window(&mut d, stage, LABELS);
        let (cli, runs) = window(&mut d, stage, &["$ cursor-agent", "Reading files", "Done"]);
        for w in [ide, cli] {
            d.set_attr(w, "class", "group absolute rounded-lg");
            d.set_attr(w, "aria-hidden", "true");
        }
        assert!(HiddenMockups::scan(&d).declared(runs[0]), "every peer hidden");
    }

    #[test]
    fn a_resting_carousel_slide_is_not_a_mockup() {
        let (mut d, body) = page();
        let track = d.add(Some(body), "div");
        d.set_rect(track, 0.0, 400.0, 1280.0, 400.0);
        let (_cur, _) = window(&mut d, track, LABELS);
        let (rest, runs) = window(&mut d, track, &["Quote", "Name", "Title", "Company"]);
        d.set_attr(rest, "aria-hidden", "true");
        d.set_attr(rest, "class", "swiper-slide");
        assert!(!HiddenMockups::scan(&d).declared(runs[0]));
    }

    #[test]
    fn page_chrome_hidden_by_a_library_or_fixed_is_not_a_mockup() {
        let (mut d, body) = page();
        let (w, runs) = window(&mut d, body, LABELS);
        d.set_attr(w, "aria-hidden", "true");
        d.set_attr(w, "data-aria-hidden", "true");
        assert!(!HiddenMockups::scan(&d).declared(runs[0]), "hidden by a modal library");
        let (mut d, body) = page();
        let (w, runs) = window(&mut d, body, LABELS);
        d.set_attr(w, "aria-hidden", "true");
        d.set_style(w, "position", "fixed");
        assert!(!HiddenMockups::scan(&d).declared(runs[0]), "a fixed bar");
        // tirionai.com: an aria-hidden intro splash.
        let (mut d, body) = page();
        let (w, runs) = window(&mut d, body, LABELS);
        d.set_attr(w, "aria-hidden", "true");
        d.set_attr(w, "class", "cine-intro");
        assert!(!HiddenMockups::scan(&d).declared(runs[0]), "an intro splash");
    }

    #[test]
    fn a_hidden_tab_panel_is_not_a_mockup() {
        let (mut d, body) = page();
        let (w, runs) = window(&mut d, body, LABELS);
        d.set_attr(w, "aria-hidden", "true");
        d.set_attr(w, "role", "tabpanel");
        assert!(!HiddenMockups::scan(&d).declared(runs[0]));
    }

    #[test]
    fn a_split_text_headline_is_not_a_mockup() {
        let (mut d, body) = page();
        let h2 = d.add(Some(body), "h2");
        d.set_rect(h2, 0.0, 400.0, 800.0, 200.0);
        let (w, runs) = window(&mut d, h2, &["Build", "the", "future", "today"]);
        d.set_attr(w, "aria-hidden", "true");
        assert!(!HiddenMockups::scan(&d).declared(runs[0]), "inside a heading");
        // A visually hidden copy of the same words beside it.
        let (mut d, body) = page();
        let (w, runs) = window(&mut d, body, &["Build", "the", "future", "today"]);
        d.set_attr(w, "aria-hidden", "true");
        let sr = d.add(Some(body), "span");
        d.set_rect(sr, 0.0, 0.0, 1.0, 1.0);
        d.add_text(sr, "Build the future today");
        assert!(!HiddenMockups::scan(&d).declared(runs[0]), "a text copy beside it");
    }

    #[test]
    fn a_labelled_screenshot_is_advisory_only() {
        let (mut d, body) = page();
        let (w, runs) = window(&mut d, body, LABELS);
        d.set_attr(w, "aria-label", "A screenshot of the Linear app showing an issue");
        let m = HiddenMockups::scan(&d);
        assert_eq!(m.signal(runs[0]), Some(MockupSignal::Labelled));
        assert!(!m.declared(runs[0]));
        // A verb is not a picture: impeccable.style's tab strip
        // "Preview Impeccable commands".
        d.set_attr(w, "aria-label", "Preview Impeccable commands");
        assert_eq!(HiddenMockups::scan(&d).signal(runs[0]), None);
        // A labelled control, or a box too small for a frame, is not one.
        d.set_attr(w, "aria-label", "Screenshot gallery");
        d.set_attr(w, "role", "tablist");
        assert_eq!(HiddenMockups::scan(&d).signal(runs[0]), None);
    }
}
