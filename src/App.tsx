import { createSignal, createEffect, For, Show, onMount } from "solid-js";
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
  const [selectedArticle, setSelectedArticle] = createSignal<Article | null>(null);
  const [feedUrl, setFeedUrl] = createSignal("");
  const [status, setStatus] = createSignal("");
  const [activePane, setActivePane] = createSignal<"feeds" | "articles" | "reader">("feeds");

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

  const selectFeed = async (feed: Feed) => {
    setSelectedFeed(feed.id);
    setSelectedArticle(null);
    setActivePane("articles");
    await loadArticles(feed.id);
  };

  const selectArticle = async (article: Article) => {
    setSelectedArticle(article);
    setActivePane("reader");
    if (!article.is_read) {
      await invoke("mark_read", { id: article.id });
      setArticles((prev) =>
        prev.map((a) => (a.id === article.id ? { ...a, is_read: true } : a))
      );
    }
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

  // Keyboard navigation
  const handleKeyDown = (e: KeyboardEvent) => {
    const pane = activePane();

    if (e.key === "Escape") {
      if (pane === "reader") setActivePane("articles");
      else if (pane === "articles") setActivePane("feeds");
      return;
    }

    if (e.key === "r" && !e.ctrlKey && !e.metaKey && document.activeElement?.tagName !== "INPUT") {
      fetchAll();
      return;
    }
  };

  onMount(() => {
    loadFeeds();
    document.addEventListener("keydown", handleKeyDown);
  });

  return (
    <div class="app" role="application" aria-label="RSS Reader">
      {/* Status bar */}
      <div class="status-bar" role="status" aria-live="polite" aria-atomic="true">
        {status()}
      </div>

      <div class="layout">
        {/* Feeds pane */}
        <nav class="pane feeds-pane" aria-label="Feed subscriptions">
          <div class="pane-header">
            <h2>Feeds</h2>
            <button onClick={fetchAll} aria-label="Refresh all feeds">
              Refresh
            </button>
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

          <ul role="listbox" aria-label="Feed list" tabindex="0">
            <For each={feeds()}>
              {(feed) => (
                <li
                  role="option"
                  aria-selected={selectedFeed() === feed.id}
                  tabindex={selectedFeed() === feed.id ? 0 : -1}
                  onClick={() => selectFeed(feed)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" || e.key === " ") {
                      e.preventDefault();
                      selectFeed(feed);
                    }
                    if (e.key === "Delete") {
                      removeFeed(feed.id);
                    }
                  }}
                >
                  <span class="feed-title">{feed.title}</span>
                </li>
              )}
            </For>
          </ul>
        </nav>

        {/* Articles pane */}
        <section class="pane articles-pane" role="complementary" aria-label="Article list">
          <div class="pane-header">
            <h2>Articles</h2>
          </div>
          <ul role="listbox" aria-label="Articles" tabindex="0">
            <For each={articles()}>
              {(article) => (
                <li
                  role="option"
                  aria-selected={selectedArticle()?.id === article.id}
                  tabindex={selectedArticle()?.id === article.id ? 0 : -1}
                  class={article.is_read ? "read" : "unread"}
                  onClick={() => selectArticle(article)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" || e.key === " ") {
                      e.preventDefault();
                      selectArticle(article);
                    }
                    if (e.key === "s") {
                      toggleStar(article.id);
                    }
                  }}
                >
                  <span class="article-status" aria-label={article.is_read ? "Read" : "Unread"}>
                    {article.is_read ? " " : "\u25CF"}
                  </span>
                  <span class="article-star" aria-label={article.is_starred ? "Starred" : ""}>
                    {article.is_starred ? "\u2605" : ""}
                  </span>
                  <span class="article-title">{article.title}</span>
                </li>
              )}
            </For>
          </ul>
        </section>

        {/* Reader pane */}
        <main class="pane reader-pane" aria-label="Article reader">
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
                  <h1>{article().title}</h1>
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
