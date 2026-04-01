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

interface Folder {
  id: number;
  name: string;
  type: string;  // "manual" | "smart"
  query: string | null;
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
  const [lastRefreshTime, setLastRefreshTime] = createSignal<number | null>(null);
  const [lastRefreshLabel, setLastRefreshLabel] = createSignal("");
  const [showAddFeed, setShowAddFeed] = createSignal(false);
  const [fullText, setFullText] = createSignal<string | null>(null);
  const [fullTextLoading, setFullTextLoading] = createSignal(false);
  const [fullTextError, setFullTextError] = createSignal<string | null>(null);
  const [folders, setFolders] = createSignal<Folder[]>([]);
  const [selectedFolder, setSelectedFolder] = createSignal<number | null>(null);

  const [expandedSections, setExpandedSections] = createSignal<Record<string, boolean>>({
    "smart-folders": true,
    "feeds": true,
  });
  const [contextMenu, setContextMenu] = createSignal<{
    x: number; y: number;
    type: "feed" | "folder";
    id: number;
  } | null>(null);
  const [contextMenuIndex, setContextMenuIndex] = createSignal(0);

  let feedListRef: HTMLUListElement | undefined;
  let articleListRef: HTMLDivElement | undefined;
  let readerRef: HTMLElement | undefined;

  const unreadCount = () => articles().filter((a) => !a.is_read).length;

  const smartFolders = () => folders().filter(f => f.type === "smart");
  const manualFolders = () => folders().filter(f => f.type === "manual");

  const toggleSection = (section: string) => {
    setExpandedSections(prev => ({ ...prev, [section]: !prev[section] }));
  };

  const isSectionExpanded = (section: string) => expandedSections()[section] !== false;

  // Get all visible treeitem elements for keyboard navigation
  const getVisibleTreeItems = (): HTMLElement[] => {
    if (!feedListRef) return [];
    return Array.from(feedListRef.querySelectorAll<HTMLElement>('[role="treeitem"]')).filter(el => {
      // An item is visible if none of its ancestor groups are collapsed
      let parent = el.parentElement;
      while (parent && parent !== feedListRef) {
        if (parent.getAttribute("role") === "group") {
          const parentItem = parent.parentElement;
          if (parentItem?.getAttribute("aria-expanded") === "false") {
            return false;
          }
        }
        parent = parent.parentElement;
      }
      return true;
    });
  };

  const focusFeedItem = (index: number) => {
    const items = getVisibleTreeItems();
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

  const loadFolders = async () => {
    try {
      const result = await invoke<Folder[]>("list_folders");
      setFolders(result);
    } catch (e) {
      // Folders not available yet — that's fine
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
      const result = await invoke<{ feed_id: number; title: string; url: string; article_count: number }>("add_feed", { url });
      setStatus(`Added: ${result.title} (${result.article_count} articles)`);
      setFeedUrl("");
      setShowAddFeed(false);
      await loadFeeds();
    } catch (e) {
      setStatus(`Error: ${e}`);
    }
  };

  const loadAllArticles = async () => {
    setSelectedFolder(null);
    setSelectedFeed(null);
    setSelectedArticle(null);
    setSelectedArticleIndex(0);
    setActivePane("articles");
    await loadArticles();
    requestAnimationFrame(() => focusArticleItem(0));
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

  // Context menu helpers
  const openContextMenu = (x: number, y: number, type: "feed" | "folder", id: number) => {
    setContextMenu({ x, y, type, id });
    setContextMenuIndex(0);
    requestAnimationFrame(() => {
      document.querySelector<HTMLElement>('[role="menu"] [role="menuitem"]')?.focus();
    });
  };

  const closeContextMenu = () => {
    setContextMenu(null);
    setContextMenuIndex(0);
  };

  const getContextMenuItems = (): Array<{ label: string; action: () => void }> => {
    const menu = contextMenu();
    if (!menu) return [];
    if (menu.type === "feed") {
      return [
        ...manualFolders().map(f => ({
          label: `Move to ${f.name}`,
          action: async () => {
            try {
              await invoke("move_feed", { feedId: menu.id, folderId: f.id });
              setStatus(`Moved feed to ${f.name}`);
              await loadFeeds();
            } catch (e) { setStatus(`Error: ${e}`); }
            closeContextMenu();
          },
        })),
        {
          label: "Uncategorize",
          action: async () => {
            try {
              await invoke("move_feed", { feedId: menu.id, folderId: null });
              setStatus("Feed uncategorized");
              await loadFeeds();
            } catch (e) { setStatus(`Error: ${e}`); }
            closeContextMenu();
          },
        },
        {
          label: "Delete feed",
          action: () => {
            removeFeed(menu.id);
            closeContextMenu();
          },
        },
      ];
    }
    if (menu.type === "folder") {
      return [
        {
          label: "Rename folder",
          action: () => {
            // TODO: implement rename_folder invoke when Tauri command exists
            setStatus("Rename folder: not yet implemented");
            closeContextMenu();
          },
        },
        {
          label: "Delete folder",
          action: async () => {
            try {
              await invoke("delete_folder", { id: menu.id });
              setStatus("Folder deleted");
              await loadFolders();
              await loadFeeds();
            } catch (e) { setStatus(`Error: ${e}`); }
            closeContextMenu();
          },
        },
      ];
    }
    return [];
  };

  const navigateContextMenu = (delta: number) => {
    const items = getContextMenuItems();
    if (items.length === 0) return;
    const newIndex = Math.max(0, Math.min(items.length - 1, contextMenuIndex() + delta));
    setContextMenuIndex(newIndex);
    requestAnimationFrame(() => {
      const menuItems = document.querySelectorAll<HTMLElement>('[role="menu"] [role="menuitem"]');
      menuItems[newIndex]?.focus();
    });
  };

  const handleContextMenuKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      closeContextMenu();
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      navigateContextMenu(1);
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      navigateContextMenu(-1);
      return;
    }
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      const items = getContextMenuItems();
      const idx = contextMenuIndex();
      if (items[idx]) items[idx].action();
      return;
    }
    if (e.key === "Tab") {
      // Trap focus inside menu
      e.preventDefault();
      return;
    }
  };

  const updateRefreshLabel = () => {
    const t = lastRefreshTime();
    if (t === null) {
      setLastRefreshLabel("");
      return;
    }
    const seconds = Math.floor((Date.now() - t) / 1000);
    if (seconds < 60) {
      setLastRefreshLabel("Last refresh: just now");
    } else {
      const minutes = Math.floor(seconds / 60);
      setLastRefreshLabel(`Last refresh: ${minutes} min ago`);
    }
  };

  const fetchAll = async () => {
    setStatus("Fetching...");
    try {
      const results = await invoke<Array<{ feed_id: number; title: string; new_articles: number; error?: string }>>("fetch_feeds");
      const statusText = results.map(r => r.error ? `${r.title}: ${r.error}` : `${r.title}: ${r.new_articles} new`).join("; ");
      setStatus(statusText);
      setLastRefreshTime(Date.now());
      updateRefreshLabel();
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
        const results = await invoke<{ imported: Array<{title: string}>; errors: Array<{title: string; error: string}>; imported_count: number }>("import_opml", { data });
        const msgs = [
          ...results.imported.map(i => `Imported: ${i.title}`),
          ...results.errors.map(e => `Error: ${e.title} - ${e.error}`),
        ];
        setStatus(msgs.join("; ") || `Imported ${results.imported_count} feeds`);
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

  const selectFolder = async (folder: Folder) => {
    setSelectedFolder(folder.id);
    setSelectedFeed(null);
    setSelectedArticle(null);
    setSelectedArticleIndex(0);
    setActivePane("articles");
    try {
      const result = await invoke<Article[]>("folder_articles", { id: folder.id });
      setArticles(result);
      setStatus(`${folder.name}: ${result.length} articles`);
    } catch (e) {
      setStatus(`Error: ${e}`);
    }
    requestAnimationFrame(() => focusArticleItem(0));
  };

  const selectFeed = async (feed: Feed, index?: number) => {
    setSelectedFolder(null);
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
    setFullText(null);
    setFullTextError(null);
    setFullTextLoading(false);
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
    // Auto-fetch full text in background (optimistic UI)
    if (article.url) {
      fetchFullText(article.id);
    }
    // Prefetch nearby articles
    if (index !== undefined) {
      prefetchNearby(index);
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

  const fetchFullText = async (articleId: number) => {
    setFullTextLoading(true);
    setFullTextError(null);
    try {
      const html = await invoke<string>("fetch_full_text", { id: articleId });
      setFullText(html);
    } catch (e) {
      setFullTextError(`${e}`);
    } finally {
      setFullTextLoading(false);
    }
  };

  const prefetchNearby = (currentIndex: number) => {
    const list = articles();
    for (const offset of [-1, 1, 2]) {
      const i = currentIndex + offset;
      if (i >= 0 && i < list.length && list[i].url) {
        // Silent prefetch — ignore errors, results cached in DB
        invoke("fetch_full_text", { id: list[i].id }).catch(() => {});
      }
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

  // Navigate visible tree items
  const navigateFeed = (delta: number) => {
    const items = getVisibleTreeItems();
    const totalItems = items.length;
    if (totalItems === 0) return;
    const current = document.activeElement as HTMLElement;
    const currentIdx = items.indexOf(current);
    const baseIdx = currentIdx >= 0 ? currentIdx : selectedFeedIndex();
    const newIndex = Math.max(0, Math.min(totalItems - 1, baseIdx + delta));
    setSelectedFeedIndex(newIndex);
    items[newIndex]?.focus();
  };

  // Global keyboard navigation
  const handleKeyDown = (e: KeyboardEvent) => {
    const tag = document.activeElement?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA") return;

    const pane = activePane();

    // Close context menu on Escape (handled first, before pane navigation)
    if (e.key === "Escape" && contextMenu()) {
      e.preventDefault();
      closeContextMenu();
      return;
    }

    if (e.key === "Escape") {
      e.preventDefault();
      if (pane === "reader") focusPane("articles");
      else if (pane === "articles") focusPane("feeds");
      return;
    }

    // Pane shortcuts: 1=feeds, 2=articles, 3=reader
    if (e.key === "1") { focusPane("feeds"); return; }
    if (e.key === "2") { focusPane("articles"); return; }
    if (e.key === "3") { focusPane("reader"); return; }

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
      else if (e.key === "ArrowRight") {
        e.preventDefault();
        const el = document.activeElement as HTMLElement;
        const expanded = el?.getAttribute("aria-expanded");
        if (expanded === "false") {
          // Expand this section
          const sectionId = el?.dataset.section;
          if (sectionId) toggleSection(sectionId);
        } else if (expanded === "true") {
          // Move to first child
          navigateFeed(1);
        }
      }
      else if (e.key === "ArrowLeft") {
        e.preventDefault();
        const el = document.activeElement as HTMLElement;
        const expanded = el?.getAttribute("aria-expanded");
        if (expanded === "true") {
          // Collapse this section
          const sectionId = el?.dataset.section;
          if (sectionId) toggleSection(sectionId);
        } else {
          // Move to parent treeitem
          const group = el?.closest('[role="group"]');
          if (group) {
            const parentItem = group.parentElement as HTMLElement;
            if (parentItem?.getAttribute("role") === "treeitem") {
              const items = getVisibleTreeItems();
              const idx = items.indexOf(parentItem);
              if (idx >= 0) {
                setSelectedFeedIndex(idx);
                parentItem.focus();
              }
            }
          }
        }
      }
      else if (e.key === "Enter") {
        e.preventDefault();
        const el = document.activeElement as HTMLElement;
        // Check if it's a section header — toggle expand
        if (el?.getAttribute("aria-expanded") !== null && el?.dataset.section) {
          toggleSection(el.dataset.section);
          return;
        }
        const feedId = el?.dataset.feedId;
        const folderId = el?.dataset.folderId;
        if (el?.id === "feed-all") {
          loadAllArticles();
        } else if (folderId) {
          const folder = folders().find(f => f.id === parseInt(folderId));
          if (folder) selectFolder(folder);
        } else if (feedId) {
          const feed = feeds().find(f => f.id === parseInt(feedId));
          if (feed) selectFeed(feed, selectedFeedIndex());
        }
      }
      else if (e.key === "Delete") {
        e.preventDefault();
        const el = document.activeElement as HTMLElement;
        const feedId = el?.dataset.feedId;
        if (feedId) {
          removeFeed(parseInt(feedId));
        }
        const folderId = el?.dataset.folderId;
        if (folderId) {
          const folder = folders().find(f => f.id === parseInt(folderId));
          if (folder && folder.type === "manual") {
            invoke("delete_folder", { id: parseInt(folderId) }).then(() => {
              setStatus("Folder deleted");
              loadFolders();
              loadFeeds();
            }).catch(e => setStatus(`Error: ${e}`));
          }
        }
      }
      else if ((e.key === "F10" && e.shiftKey) || e.key === "ContextMenu") {
        // Open context menu at current item position
        e.preventDefault();
        const el = document.activeElement as HTMLElement;
        if (el) {
          const rect = el.getBoundingClientRect();
          const feedId = el.dataset.feedId;
          const folderId = el.dataset.folderId;
          if (feedId) {
            openContextMenu(rect.left + 20, rect.bottom, "feed", parseInt(feedId));
          } else if (folderId) {
            // Only manual folders get context menus
            const folder = folders().find(f => f.id === parseInt(folderId));
            if (folder && folder.type === "manual") {
              openContextMenu(rect.left + 20, rect.bottom, "folder", parseInt(folderId));
            }
          }
        }
      }
      else if (e.key === "F2") {
        e.preventDefault();
        // TODO: implement inline rename when rename_folder command exists
        setStatus("Rename: not yet implemented (F2)");
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
    loadFolders();
    document.addEventListener("keydown", handleKeyDown);

    // Close context menu on click outside
    const handleClickOutside = (e: MouseEvent) => {
      if (contextMenu() && !(e.target as HTMLElement)?.closest('[role="menu"]')) {
        closeContextMenu();
      }
    };
    document.addEventListener("click", handleClickOutside);

    const autoRefreshInterval = setInterval(() => {
      fetchAll();
    }, 15 * 60 * 1000);

    const labelUpdateInterval = setInterval(() => {
      updateRefreshLabel();
    }, 30 * 1000);

    onCleanup(() => {
      document.removeEventListener("keydown", handleKeyDown);
      document.removeEventListener("click", handleClickOutside);
      clearInterval(autoRefreshInterval);
      clearInterval(labelUpdateInterval);
    });
  });

  return (
    <div class="app" aria-label="RSS Reader">
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
        <span>{status()}</span>
        <Show when={lastRefreshLabel()}>
          <span class="refresh-indicator">{lastRefreshLabel()}</span>
        </Show>
      </div>

      <div class="layout">
        {/* Feeds pane */}
        <nav class="pane feeds-pane" aria-label="Feed subscriptions">
          <div class="pane-header">
            <h2>Feeds</h2>
            <div class="pane-actions">
              <button
                onClick={() => {
                  setShowAddFeed(!showAddFeed());
                  if (!showAddFeed()) {
                    requestAnimationFrame(() => document.getElementById("feed-url")?.focus());
                  }
                }}
                aria-label="Add feed"
                aria-expanded={showAddFeed()}
              >
                + Add
              </button>
              <button onClick={fetchAll} aria-label="Refresh all feeds">
                Refresh
              </button>
            </div>
          </div>

          <Show when={showAddFeed()}>
            <div class="feed-management" aria-label="Feed management">
              <form
                class="add-feed-form"
                onSubmit={(e) => { e.preventDefault(); addFeed(); }}
                aria-label="Add new feed"
              >
                <label for="feed-url" class="sr-only">Feed URL</label>
                <input
                  id="feed-url"
                  type="url"
                  placeholder="Paste feed URL, then Enter"
                  value={feedUrl()}
                  onInput={(e) => setFeedUrl(e.currentTarget.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Escape") {
                      setShowAddFeed(false);
                      setFeedUrl("");
                    }
                  }}
                  aria-label="Feed URL"
                />
                <button type="submit" aria-label="Confirm add feed">OK</button>
                <button type="button" onClick={() => { setShowAddFeed(false); setFeedUrl(""); }} aria-label="Cancel">
                  Cancel
                </button>
              </form>
              <div class="opml-actions">
                <button onClick={importOpml} aria-label="Import feeds from OPML file">Import OPML</button>
                <button onClick={exportOpml} aria-label="Export feeds as OPML file">Export OPML</button>
              </div>
            </div>
          </Show>

          <ul
            ref={feedListRef}
            role="tree"
            aria-label="Feed subscriptions"
            tabindex={-1}
          >
            {/* All Articles — top-level treeitem */}
            <li
              id="feed-all"
              role="treeitem"
              class="all-articles-item feed-button"
              tabindex={selectedFeedIndex() === 0 ? 0 : -1}
              aria-selected={selectedFeed() === null && selectedFolder() === null}
              onClick={() => loadAllArticles()}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  loadAllArticles();
                }
              }}
            >
              <span class="feed-title">All Articles</span>
            </li>

            {/* Smart Folders section */}
            <Show when={smartFolders().length > 0}>
              <li
                role="treeitem"
                aria-expanded={isSectionExpanded("smart-folders")}
                data-section="smart-folders"
                class="section-header feed-button"
                tabindex={-1}
                onClick={() => toggleSection("smart-folders")}
              >
                <span class="section-toggle" aria-hidden="true">
                  {isSectionExpanded("smart-folders") ? "\u25BE" : "\u25B8"}
                </span>
                <span class="feed-title">Smart Folders</span>
              </li>
              <Show when={isSectionExpanded("smart-folders")}>
                <li role="none">
                  <ul role="group">
                    <For each={smartFolders()}>
                      {(folder) => (
                        <li
                          role="treeitem"
                          class="feed-button tree-child"
                          tabindex={-1}
                          aria-selected={selectedFolder() === folder.id}
                          aria-label={`${folder.name} smart folder`}
                          data-folder-id={folder.id}
                          onClick={() => selectFolder(folder)}
                          onKeyDown={(e) => {
                            if (e.key === "Enter" || e.key === " ") {
                              e.preventDefault();
                              selectFolder(folder);
                            }
                          }}
                        >
                          <span class="folder-icon" aria-hidden="true">{"\uD83D\uDCC1"}</span>
                          <span class="feed-title">{folder.name}</span>
                        </li>
                      )}
                    </For>
                  </ul>
                </li>
              </Show>
            </Show>

            {/* Feeds section */}
            <li
              role="treeitem"
              aria-expanded={isSectionExpanded("feeds")}
              data-section="feeds"
              class="section-header feed-button"
              tabindex={-1}
              onClick={() => toggleSection("feeds")}
            >
              <span class="section-toggle" aria-hidden="true">
                {isSectionExpanded("feeds") ? "\u25BE" : "\u25B8"}
              </span>
              <span class="feed-title">Feeds</span>
            </li>
            <Show when={isSectionExpanded("feeds")}>
              <li role="none">
                <ul role="group">
                  <For each={feeds()}>
                    {(feed) => {
                      const feedUnread = () => articles().filter((a) => a.feed_id === feed.id && !a.is_read).length;
                      return (
                        <li
                          id={`feed-${feed.id}`}
                          role="treeitem"
                          class="feed-button tree-child"
                          tabindex={-1}
                          aria-selected={selectedFeed() === feed.id}
                          aria-label={`${feed.title}${feedUnread() > 0 ? `, ${feedUnread()} unread` : ""}`}
                          data-feed-id={feed.id}
                          onClick={() => selectFeed(feed)}
                          onContextMenu={(e) => {
                            e.preventDefault();
                            openContextMenu(e.clientX, e.clientY, "feed", feed.id);
                          }}
                          onKeyDown={(e) => {
                            if (e.key === "Delete") {
                              removeFeed(feed.id);
                            }
                            if (e.key === "Enter" || e.key === " ") {
                              e.preventDefault();
                              selectFeed(feed);
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
              </li>
            </Show>
          </ul>
        </nav>

        {/* Context menu */}
        <Show when={contextMenu()}>
          {(menu) => (
            <ul
              role="menu"
              class="context-menu"
              aria-label="Context menu"
              style={{
                position: "fixed",
                left: `${menu().x}px`,
                top: `${menu().y}px`,
              }}
              onKeyDown={handleContextMenuKeyDown}
            >
              <For each={getContextMenuItems()}>
                {(item, index) => (
                  <li
                    role="menuitem"
                    class="context-menu-item"
                    tabindex={contextMenuIndex() === index() ? 0 : -1}
                    onClick={(e) => {
                      e.stopPropagation();
                      item.action();
                    }}
                    onFocus={() => setContextMenuIndex(index())}
                  >
                    {item.label}
                  </li>
                )}
              </For>
            </ul>
          )}
        </Show>

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
          <div
            ref={articleListRef}
            role="listbox"
            aria-label="Articles"
            tabindex={-1}
          >
            <For each={articles()}>
              {(article, index) => {
                const titleId = `article-title-${article.id}`;
                return (
                  <div
                    id={`article-${article.id}`}
                    role="option"
                    aria-selected={selectedArticle()?.id === article.id}
                    aria-label={`${article.title}, ${article.is_read ? "read" : "unread"}${article.is_starred ? ", starred" : ""}`}
                    tabindex={selectedArticleIndex() === index() ? 0 : -1}
                    class={`article-item ${article.is_read ? "read" : "unread"}`}
                    onClick={() => selectArticle(article, index())}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        selectArticle(article, index());
                      }
                      if (e.key === "s") {
                        toggleStar(article.id);
                      }
                      if (e.key === "ArrowDown" || e.key === "j") {
                        e.preventDefault();
                        navigateArticle(1);
                      }
                      if (e.key === "ArrowUp" || e.key === "k") {
                        e.preventDefault();
                        navigateArticle(-1);
                      }
                    }}
                    onFocus={() => setSelectedArticleIndex(index())}
                  >
                    <span class="article-status" aria-hidden="true">
                      {article.is_read ? " " : "\u25CF"}
                    </span>
                    <span class="article-star" aria-hidden="true">
                      {article.is_starred ? "\u2605" : ""}
                    </span>
                    <span id={titleId} class="article-title">{article.title}</span>
                  </div>
                );
              }}
            </For>
          </div>
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
                    <Show when={fullTextError()}>
                      <span class="error-text" role="alert">{fullTextError()}</span>
                    </Show>
                  </div>
                </header>
                <div
                  class="article-content"
                  innerHTML={fullText() || article().content || article().summary || "<p>No content available.</p>"}
                />
              </article>
            )}
          </Show>
        </main>
      </div>
    </div>
  );
}
