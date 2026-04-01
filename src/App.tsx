import { createSignal, createEffect, For, Show, onMount, onCleanup } from "solid-js";
import { invoke } from "@tauri-apps/api/core";

interface Feed {
  id: number;
  title: string;
  url: string;
  site_url: string | null;
  description: string | null;
  added_at: string;
}

interface Article {
  id: number;
  feed_id: number;
  title: string;
  url: string | null;
  content: string | null;
  summary: string | null;
  published_at: string | null;
  is_read: boolean;
  is_starred: boolean;
  fetched_at: string;
}

export default function App() {
  const [feeds, setFeeds] = createSignal<Feed[]>([]);
  const [articles, setArticles] = createSignal<Article[]>([]);
  const [selectedFeed, setSelectedFeed] = createSignal<number | null>(null);
  const [selectedFeedIndex, setSelectedFeedIndex] = createSignal(0);
  const [selectedArticle, setSelectedArticle] = createSignal<Article | null>(null);
  const [selectedArticleIndex, setSelectedArticleIndex] = createSignal(0);
  const [feedUrl, setFeedUrl] = createSignal("");
  const [status, setStatus] = createSignal("");
  const [activePane, setActivePane] = createSignal<"feeds" | "articles" | "reader">("feeds");
  const [articleAnnouncement, setArticleAnnouncement] = createSignal("");
  const [searchQuery, setSearchQuery] = createSignal("");

  let feedListRef: HTMLUListElement | undefined;
  let articleListRef: HTMLUListElement | undefined;
  let readerRef: HTMLElement | undefined;

  const unreadCount = () => articles().filter((a) => !a.is_read).length;

  const focusFeedItem = (index: number) => {
    const items = feedListRef?.querySelectorAll<HTMLElement>('[role="option"]');
    if (items && items[index]) {
      items[index].focus();
    }
  };

  const focusArticleItem = (index: number) => {
    const items = articleListRef?.querySelectorAll<HTMLElement>('[role="option"]');
    if (items && items[index]) {
      items[index].focus();
    }
  };

  const focusPane = (pane: "feeds" | "articles" | "reader") => {
    setActivePane(pane);
    requestAnimationFrame(() => {
      if (pane === "feeds") {
        focusFeedItem(selectedFeedIndex());
      } else if (pane === "articles") {
        focusArticleItem(selectedArticleIndex());
      } else if (pane === "reader" && readerRef) {
        readerRef.focus();
      }
    });
  };

  const loadFeeds = async () => {
    try {
      const result = await invoke<Feed[]>("list_feeds");
      setFeeds(result);
    } catch (e) {
      setStatus(`Error: ${e}`);
    }
  };

  const loadArticles = async (feedId?: number) => {
    try {
      const result = await invoke<Article[]>("list_articles", {
        feedId: feedId ?? null,
        unreadOnly: false,
      });
      setArticles(result);
      const total = result.length;
      const unread = result.filter((a) => !a.is_read).length;
      setStatus(`${total} articles, ${unread} unread`);
    } catch (e) {
      setStatus(`Error: ${e}`);
    }
  };

  const addFeed = async () => {
    const url = feedUrl().trim();
    if (!url) return;
    setStatus("Adding feed...");
    try {
      const result = await invoke<{ feed: Feed; article_count: number }>("add_feed", { url });
      setStatus(`Added: ${result.feed.title} (${result.article_count} articles)`);
      setFeedUrl("");
      await loadFeeds();
    } catch (e) {
      setStatus(`Error: ${e}`);
    }
  };

  const removeFeed = async (id: number) => {
    try {
      await invoke("remove_feed", { id });
      await loadFeeds();
      if (selectedFeed() === id) {
        setSelectedFeed(null);
        setArticles([]);
        setSelectedArticle(null);
      }
      setStatus("Feed removed.");
    } catch (e) {
      setStatus(`Error: ${e}`);
    }
  };

  const fetchAll = async () => {
    setStatus("Fetching...");
    try {
      const results = await invoke<string[]>("fetch_feeds");
      setStatus(results.join("; "));
      const fid = selectedFeed();
      await loadArticles(fid ?? undefined);
    } catch (e) {
      setStatus(`Error: ${e}`);
    }
  };

  const searchArticles = async (query: string) => {
    if (!query.trim()) {
      const fid = selectedFeed();
      await loadArticles(fid ?? undefined);
      return;
    }
    try {
      const result = await invoke<Article[]>("search_articles", { query });
      setArticles(result);
      setStatus(`${result.length} results for "${query}"`);
    } catch (e) {
      setStatus(`Error: ${e}`);
    }
  };

  const importOpml = () => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".opml,.xml";
    input.onchange = async () => {
      const file = input.files?.[0];
      if (!file) return;
      const data = await file.text();
      setStatus("Importing...");
      try {
        const results = await invoke<string[]>("import_opml", { data });
        setStatus(results.join("; "));
        await loadFeeds();
      } catch (e) {
        setStatus(`Error: ${e}`);
      }
    };
    input.click();
  };

  const exportOpml = async () => {
    try {
      const opml = await invoke<string>("export_opml");
      const blob = new Blob([opml], { type: "application/xml" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = "feeds.opml";
      a.click();
      URL.revokeObjectURL(url);
      setStatus("Exported OPML.");
    } catch (e) {
      setStatus(`Error: ${e}`);
    }
  };

  const selectFeed = async (feed: Feed, index?: number) => {
    setSelectedFeed(feed.id);
    if (index !== undefined) setSelectedFeedIndex(index);
    setSelectedArticle(null);
    setSelectedArticleIndex(0);
    setActivePane("articles");
    await loadArticles(feed.id);
    requestAnimationFrame(() => focusArticleItem(0));
  };

  const selectArticle = async (article: Article, index?: number) => {
    setSelectedArticle(article);
    if (index !== undefined) setSelectedArticleIndex(index);
    setActivePane("reader");
    setArticleAnnouncement(
      `${article.title}, ${article.is_read ? "read" : "unread"}`
    );
    if (!article.is_read) {
      await invoke("mark_read", { id: article.id });
      setArticles((prev) =>
        prev.map((a) => (a.id === article.id ? { ...a, is_read: true } : a))
      );
    }
    requestAnimationFrame(() => readerRef?.focus());
  };

  const toggleStar = async (id: number) => {
    await invoke("toggle_star", { id });
    setArticles((prev) =>
      prev.map((a) => (a.id === id ? { ...a, is_starred: !a.is_starred } : a))
    );
    const sel = selectedArticle();
    if (sel && sel.id === id) {
      setSelectedArticle({ ...sel, is_starred: !sel.is_starred });
    }
  };

  const navigateArticle = (delta: number) => {
    const list = articles();
    if (!list.length) return;
    const newIndex = Math.max(0, Math.min(list.length - 1, selectedArticleIndex() + delta));
    setSelectedArticleIndex(newIndex);
    focusArticleItem(newIndex);
  };

  const navigateUnread = (direction: "next" | "prev") => {
    const list = articles();
    const current = selectedArticleIndex();
    if (direction === "next") {
      for (let i = current + 1; i < list.length; i++) {
        if (!list[i].is_read) { setSelectedArticleIndex(i); focusArticleItem(i); return; }
      }
    } else {
      for (let i = current - 1; i >= 0; i--) {
        if (!list[i].is_read) { setSelectedArticleIndex(i); focusArticleItem(i); return; }
      }
    }
  };

  const navigateFeed = (delta: number) => {
    const list = feeds();
    if (!list.length) return;
    const newIndex = Math.max(0, Math.min(list.length - 1, selectedFeedIndex() + delta));
    setSelectedFeedIndex(newIndex);
    focusFeedItem(newIndex);
  };

  // Global keyboard navigation
  const handleKeyDown = (e: KeyboardEvent) => {
    const tag = document.activeElement?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA") return;

    const pane = activePane();

    if (e.key === "Escape") {
      e.preventDefault();
      if (pane === "reader") focusPane("articles");
      else if (pane === "articles") focusPane("feeds");
      return;
    }

    if (e.key === "Tab" && !e.shiftKey && !e.ctrlKey) {
      e.preventDefault();
      if (pane === "feeds") focusPane("articles");
      else if (pane === "articles") focusPane("reader");
      else focusPane("feeds");
      return;
    }
    if (e.key === "Tab" && e.shiftKey && !e.ctrlKey) {
      e.preventDefault();
      if (pane === "reader") focusPane("articles");
      else if (pane === "articles") focusPane("feeds");
      else focusPane("reader");
      return;
    }

    if (e.key === "h") {
      focusPane("feeds");
      return;
    }

    if (e.key === "r" && !e.ctrlKey && !e.metaKey) {
      fetchAll();
      return;
    }

    if (pane === "feeds") {
      if (e.key === "ArrowDown") { e.preventDefault(); navigateFeed(1); }
      else if (e.key === "ArrowUp") { e.preventDefault(); navigateFeed(-1); }
      else if (e.key === "Enter") {
        e.preventDefault();
        const list = feeds();
        const idx = selectedFeedIndex();
        if (list[idx]) selectFeed(list[idx], idx);
      }
    }

    if (pane === "articles") {
      if (e.key === "j" || e.key === "ArrowDown") { e.preventDefault(); navigateArticle(1); }
      else if (e.key === "k" || e.key === "ArrowUp") { e.preventDefault(); navigateArticle(-1); }
      else if (e.key === "Enter") {
        e.preventDefault();
        const list = articles();
        const idx = selectedArticleIndex();
        if (list[idx]) selectArticle(list[idx], idx);
      }
      else if (e.key === "n") { navigateUnread("next"); }
      else if (e.key === "p") { navigateUnread("prev"); }
    }

    if (pane === "reader") {
      if (e.key === "j") { e.preventDefault(); navigateArticle(1); const a = articles()[selectedArticleIndex()]; if (a) selectArticle(a, selectedArticleIndex()); }
      else if (e.key === "k") { e.preventDefault(); navigateArticle(-1); const a = articles()[selectedArticleIndex()]; if (a) selectArticle(a, selectedArticleIndex()); }
      else if (e.key === "n") { navigateUnread("next"); const a = articles()[selectedArticleIndex()]; if (a) selectArticle(a, selectedArticleIndex()); }
      else if (e.key === "p") { navigateUnread("prev"); const a = articles()[selectedArticleIndex()]; if (a) selectArticle(a, selectedArticleIndex()); }
    }
  };

  onMount(() => {
    loadFeeds();
    document.addEventListener("keydown", handleKeyDown);
  });

  onCleanup(() => {
    document.removeEventListener("keydown", handleKeyDown);
  });

  return (
    <div class="app" role="application" aria-label="RSS Reader">
      {/* Visually hidden app heading for screen readers */}
      <h1 class="sr-only">RSS Reader</h1>

      {/* Skip navigation link */}
      <a href="#reader-pane" class="skip-link">Skip to main content</a>

      {/* Live region for article announcements */}
      <div class="sr-only" aria-live="assertive" aria-atomic="true">
        {articleAnnouncement()}
      </div>

      {/* Status bar */}
      <div class="status-bar" role="status" aria-live="polite" aria-atomic="true">
        {status()}
      </div>

      <div class="layout">
        {/* Feeds pane */}
        <nav class="pane feeds-pane" aria-label="Feed subscriptions">
          <div class="pane-header">
            <h2>Feeds</h2>
            <div class="pane-actions">
              <button onClick={importOpml} aria-label="Import feeds from OPML file">
                Import
              </button>
              <button onClick={exportOpml} aria-label="Export feeds as OPML file">
                Export
              </button>
              <button onClick={fetchAll} aria-label="Refresh all feeds">
                Refresh
              </button>
            </div>
          </div>

          <form
            class="add-feed-form"
            onSubmit={(e) => { e.preventDefault(); addFeed(); }}
            role="search"
            aria-label="Add new feed"
          >
            <label for="feed-url" class="sr-only">Feed URL</label>
            <input
              id="feed-url"
              type="url"
              placeholder="Feed URL"
              value={feedUrl()}
              onInput={(e) => setFeedUrl(e.currentTarget.value)}
              aria-label="Feed URL"
            />
            <button type="submit">Add</button>
          </form>

          <ul
            ref={feedListRef}
            role="listbox"
            aria-label="Feed list"
            tabindex="0"
            aria-activedescendant={selectedFeed() ? `feed-${selectedFeed()}` : undefined}
          >
            <For each={feeds()}>
              {(feed, index) => {
                const feedUnread = () => articles().filter((a) => a.feed_id === feed.id && !a.is_read).length;
                return (
                  <li
                    id={`feed-${feed.id}`}
                    role="option"
                    aria-selected={selectedFeed() === feed.id}
                    aria-label={`${feed.title}${feedUnread() > 0 ? `, ${feedUnread()} unread` : ""}`}
                    tabindex={selectedFeedIndex() === index() ? 0 : -1}
                    onClick={() => selectFeed(feed, index())}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        selectFeed(feed, index());
                      }
                      if (e.key === "Delete") {
                        removeFeed(feed.id);
                      }
                      if (e.key === "ArrowDown") {
                        e.preventDefault();
                        navigateFeed(1);
                      }
                      if (e.key === "ArrowUp") {
                        e.preventDefault();
                        navigateFeed(-1);
                      }
                    }}
                  >
                    <span class="feed-title">{feed.title}</span>
                    <Show when={feedUnread() > 0}>
                      <span class="unread-badge" aria-hidden="true">{feedUnread()}</span>
                    </Show>
                  </li>
                );
              }}
            </For>
          </ul>
        </nav>

        {/* Articles pane */}
        <section
          class="pane articles-pane"
          role="complementary"
          aria-label="Article list"
        >
          <div class="pane-header">
            <h2>Articles</h2>
          </div>
          <div role="search" aria-label="Search articles" class="search-bar">
            <label for="search-input" class="sr-only">Search articles</label>
            <input
              id="search-input"
              type="search"
              placeholder="Search..."
              value={searchQuery()}
              onInput={(e) => {
                const q = e.currentTarget.value;
                setSearchQuery(q);
                searchArticles(q);
              }}
              aria-label="Search articles"
            />
          </div>
          <ul
            ref={articleListRef}
            role="listbox"
            aria-roledescription="article list"
            aria-label="Articles"
            tabindex="0"
            aria-activedescendant={selectedArticle() ? `article-${selectedArticle()!.id}` : undefined}
          >
            <For each={articles()}>
              {(article, index) => (
                <li
                  id={`article-${article.id}`}
                  role="option"
                  aria-selected={selectedArticleIndex() === index()}
                  aria-label={`${article.title}, ${article.is_read ? "read" : "unread"}${article.is_starred ? ", starred" : ""}`}
                  tabindex={selectedArticleIndex() === index() ? 0 : -1}
                  class={article.is_read ? "read" : "unread"}
                  onClick={() => selectArticle(article, index())}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" || e.key === " ") {
                      e.preventDefault();
                      selectArticle(article, index());
                    }
                    if (e.key === "s") {
                      toggleStar(article.id);
                    }
                  }}
                >
                  <span class="article-status" aria-hidden="true">
                    {article.is_read ? " " : "\u25CF"}
                  </span>
                  <span class="article-star" aria-hidden="true">
                    {article.is_starred ? "\u2605" : ""}
                  </span>
                  <span class="article-title">{article.title}</span>
                </li>
              )}
            </For>
          </ul>
        </section>

        {/* Reader pane */}
        <main
          id="reader-pane"
          class="pane reader-pane"
          aria-label="Article reader"
          ref={readerRef}
          tabindex="-1"
        >
          <Show
            when={selectedArticle()}
            fallback={
              <div class="empty-state" role="status">
                <p>Select an article to read</p>
              </div>
            }
          >
            {(article) => (
              <article aria-label={article().title}>
                <header>
                  <h3>{article().title}</h3>
                  <div class="article-meta">
                    <Show when={article().url}>
                      {(url) => (
                        <a href={url()} target="_blank" rel="noopener noreferrer" aria-label="Open original article">
                          Original
                        </a>
                      )}
                    </Show>
                    <button
                      onClick={() => toggleStar(article().id)}
                      aria-label={article().is_starred ? "Remove star" : "Star this article"}
                      aria-pressed={article().is_starred}
                    >
                      {article().is_starred ? "\u2605 Starred" : "\u2606 Star"}
                    </button>
                  </div>
                </header>
                <div
                  class="article-content"
                  innerHTML={article().content || article().summary || "<p>No content available.</p>"}
                />
              </article>
            )}
          </Show>
        </main>
      </div>
    </div>
  );
}
