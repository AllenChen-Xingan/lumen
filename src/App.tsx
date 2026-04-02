import { createSignal, createEffect, For, Show, onMount, onCleanup } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface Feed {
  id: number;
  title: string;
  url: string;
  site_url: string | null;
  description: string | null;
  added_at: string;
  folder_id: number | null;
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
  tldr: string | null;
  guid: string | null;
  full_content: string | null;
  tags: string | null;
}

interface Folder {
  id: number | null;
  name: string;
  type: string;  // "smart_view" | "manual" | "smart"
  query: string | null;
  article_count: number | null;
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
  const [selectedFolder, setSelectedFolder] = createSignal<string | null>(null);
  const [feedBusy, setFeedBusy] = createSignal(false);
  const [hasMore, setHasMore] = createSignal(false);
  const [articleOffset, setArticleOffset] = createSignal(0);
  const PAGE_SIZE = 50;
  const [showNewFolder, setShowNewFolder] = createSignal(false);
  const [newFolderName, setNewFolderName] = createSignal("");
  const [pendingMoveFeedId, setPendingMoveFeedId] = createSignal<number | null>(null);
  const [manageMode, setManageMode] = createSignal(false);
  const [selectedFeedIds, setSelectedFeedIds] = createSignal<Set<number>>(new Set());
  const [bulkTargetFolder, setBulkTargetFolder] = createSignal<number | null>(null);
  const [sortOrder, setSortOrder] = createSignal<"newest" | "oldest">("newest");

  const [expandedSections, setExpandedSections] = createSignal<Record<string, boolean>>({
    "smart-folders": true,
    "manual-folders": true,
    "feeds": true,
    // Per-folder keys like "folder-5" are added dynamically
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
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  let contextMenuTrigger: HTMLElement | null = null;
  const fetchedArticleIds = new Set<number>();

  const unreadCount = () => articles().filter((a) => !a.is_read).length;

  const formatDate = (dateStr: string | null): string => {
    if (!dateStr) return "";
    const d = new Date(dateStr);
    if (isNaN(d.getTime())) return "";
    const now = Date.now();
    const diffMs = now - d.getTime();
    const diffMin = Math.floor(diffMs / 60000);
    if (diffMin < 1) return "just now";
    if (diffMin < 60) return `${diffMin}m ago`;
    const diffHrs = Math.floor(diffMin / 60);
    if (diffHrs < 24) return `${diffHrs}h ago`;
    const diffDays = Math.floor(diffHrs / 24);
    if (diffDays < 7) return `${diffDays}d ago`;
    return d.toLocaleDateString();
  };

  const cognitiveFolders = () => folders().filter(f => f.type === "smart_view");
  const manualFolders = () => folders().filter(f => f.type === "manual");
  const uncategorizedFeeds = () => feeds().filter(f => f.folder_id === null);

  const sortedArticles = () => {
    const list = [...articles()];
    if (sortOrder() === "oldest") {
      list.sort((a, b) => {
        const ta = a.published_at || a.fetched_at;
        const tb = b.published_at || b.fetched_at;
        return ta.localeCompare(tb);
      });
    } else {
      list.sort((a, b) => {
        const ta = a.published_at || a.fetched_at;
        const tb = b.published_at || b.fetched_at;
        return tb.localeCompare(ta);
      });
    }
    return list;
  };
  const feedsInFolder = (folderId: number) => feeds().filter(f => f.folder_id === folderId);

  const toggleSection = (section: string) => {
    setExpandedSections(prev => ({ ...prev, [section]: !prev[section] }));
  };

  const isSectionExpanded = (section: string) => expandedSections()[section] !== false;

  // Find the section header treeitem that owns a role="group" container.
  // DOM structure: <li role="treeitem" aria-expanded> followed by <li role="none"><ul role="group">
  // So the header is the previousElementSibling of the role="none" wrapper.
  const getSectionHeader = (group: HTMLElement): HTMLElement | null => {
    const wrapper = group.parentElement;
    if (wrapper?.getAttribute("role") === "none") {
      const sibling = wrapper.previousElementSibling as HTMLElement | null;
      if (sibling?.getAttribute("role") === "treeitem" && sibling.hasAttribute("aria-expanded")) {
        return sibling;
      }
    }
    return null;
  };

  // Get all visible treeitem elements for keyboard navigation
  const getVisibleTreeItems = (): HTMLElement[] => {
    if (!feedListRef) return [];
    return Array.from(feedListRef.querySelectorAll<HTMLElement>('[role="treeitem"]')).filter(el => {
      let parent = el.parentElement;
      while (parent && parent !== feedListRef) {
        if (parent.getAttribute("role") === "group") {
          const header = getSectionHeader(parent);
          if (header?.getAttribute("aria-expanded") === "false") {
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
    const items = articleListRef?.querySelectorAll<HTMLElement>('article');
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
        const heading = readerRef.querySelector('h3');
        if (heading) {
          (heading as HTMLElement).tabIndex = -1;
          (heading as HTMLElement).focus();
        } else {
          readerRef.focus();
        }
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
      // Folders not available yet
    }
  };

  const loadArticles = async (feedId?: number, append?: boolean) => {
    setFeedBusy(true);
    const offset = append ? articleOffset() : 0;
    if (!append) {
      setArticleOffset(0);
    }
    try {
      const result = await invoke<{ articles: Article[]; has_more: boolean; offset: number; count: number }>("list_articles", {
        feedId: feedId ?? null,
        unreadOnly: false,
        count: PAGE_SIZE,
        offset,
      });
      if (append) {
        setArticles(prev => [...prev, ...result.articles]);
      } else {
        setArticles(result.articles);
      }
      setHasMore(result.has_more);
      setArticleOffset(offset + result.articles.length);
      const total = articles().length;
      const unread = articles().filter((a) => !a.is_read).length;
      setStatus(`${total} articles${result.has_more ? "+" : ""}, ${unread} unread`);
    } catch (e) {
      setStatus(`Error: ${e}`);
    }
    setFeedBusy(false);
  };

  const loadMoreArticles = async () => {
    const feedId = selectedFeed();
    await loadArticles(feedId ?? undefined, true);
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
    setSearchQuery("");
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

  const createNewFolder = async (name: string, moveFeedId?: number) => {
    const trimmed = name.trim();
    if (!trimmed) return;
    try {
      const folder = await invoke<{ id: number; name: string }>("create_folder", { name: trimmed });
      setStatus(`Created folder: ${folder.name}`);
      if (moveFeedId) {
        await invoke("move_feed", { feedId: moveFeedId, folderId: folder.id });
        setStatus(`Created "${folder.name}" and moved feed`);
      }
      await loadFolders();
      await loadFeeds();
    } catch (e) {
      setStatus(`Error: ${e}`);
    }
    setNewFolderName("");
    setShowNewFolder(false);
    setPendingMoveFeedId(null);
  };

  const toggleFeedSelection = (feedId: number) => {
    setSelectedFeedIds(prev => {
      const next = new Set(prev);
      if (next.has(feedId)) next.delete(feedId);
      else next.add(feedId);
      return next;
    });
  };

  const bulkMoveFeeds = async (folderId: number | null) => {
    const ids = Array.from(selectedFeedIds());
    if (ids.length === 0) return;
    try {
      for (const feedId of ids) {
        await invoke("move_feed", { feedId, folderId });
      }
      setStatus(`Moved ${ids.length} feeds`);
      setSelectedFeedIds(new Set<number>());
      setManageMode(false);
      await loadFolders();
      await loadFeeds();
    } catch (e) {
      setStatus(`Error: ${e}`);
    }
  };

  // Context menu helpers
  const openContextMenu = (x: number, y: number, type: "feed" | "folder", id: number) => {
    contextMenuTrigger = document.activeElement as HTMLElement;
    setContextMenu({ x, y, type, id });
    setContextMenuIndex(0);
    requestAnimationFrame(() => {
      document.querySelector<HTMLElement>('[role="menu"] [role="menuitem"]')?.focus();
    });
  };

  const closeContextMenu = () => {
    setContextMenu(null);
    setContextMenuIndex(0);
    requestAnimationFrame(() => contextMenuTrigger?.focus());
    contextMenuTrigger = null;
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
              await loadFolders();
            } catch (e) { setStatus(`Error: ${e}`); }
            closeContextMenu();
          },
        })),
        {
          label: "New folder...",
          action: () => {
            setPendingMoveFeedId(menu.id);
            setShowNewFolder(true);
            closeContextMenu();
            requestAnimationFrame(() => document.getElementById("new-folder-name")?.focus());
          },
        },
        // Only show Uncategorize if the feed is currently in a folder
        ...(feeds().find(f => f.id === menu.id)?.folder_id ? [{
          label: "Uncategorize",
          action: async () => {
            try {
              await invoke("move_feed", { feedId: menu.id, folderId: null });
              setStatus("Feed uncategorized");
              await loadFeeds();
              await loadFolders();
            } catch (e) { setStatus(`Error: ${e}`); }
            closeContextMenu();
          },
        }] : []),
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
            // TODO: rename not yet in CLI — just close for now
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
    if (e.code === "Tab") {
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
    setFeedBusy(true);
    setStatus("Fetching feeds...");
    try {
      // fetch_feeds now returns immediately; results arrive via fetch-complete event
      await invoke("fetch_feeds");
    } catch (e) {
      setStatus(`Error: ${e}`);
      setFeedBusy(false);
    }
  };

  const searchArticles = async (query: string) => {
    if (!query.trim()) {
      const fid = selectedFeed();
      await loadArticles(fid ?? undefined);
      return;
    }
    setFeedBusy(true);
    try {
      const result = await invoke<Article[]>("search_articles", { query });
      setArticles(result);
      setStatus(`${result.length} results for "${query}"`);
    } catch (e) {
      setStatus(`Error: ${e}`);
    }
    setFeedBusy(false);
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

  const selectCognitiveFolder = async (folderName: string) => {
    setSelectedFolder(folderName);
    setSelectedFeed(null);
    setSelectedArticle(null);
    setSelectedArticleIndex(0);
    setActivePane("articles");
    setFeedBusy(true);
    try {
      const result = await invoke<Article[]>("folder_articles", { tag: folderName });
      setArticles(result);
      setStatus(`${folderName}: ${result.length} articles`);
    } catch (e) {
      setStatus(`Error: ${e}`);
    }
    setFeedBusy(false);
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
    if (article.url && !article.full_content && !fetchedArticleIds.has(article.id)) {
      fetchedArticleIds.add(article.id);
      fetchFullText(article.id);
    }
    requestAnimationFrame(() => {
      // Focus the article heading so NVDA enters browse mode for reading
      const heading = readerRef?.querySelector('h3');
      if (heading) {
        (heading as HTMLElement).tabIndex = -1;
        (heading as HTMLElement).focus();
      } else {
        readerRef?.focus();
      }
    });
  };

  const toggleStar = async (id: number) => {
    await invoke("toggle_star", { id });
    setArticles((prev) =>
      prev.map((a) => (a.id === id ? { ...a, is_starred: !a.is_starred } : a))
    );
    const updated = articles().find(a => a.id === id);
    if (updated) {
      setSelectedArticle({ ...updated });
    }
  };

  const fetchFullText = async (articleId: number) => {
    setFullTextLoading(true);
    setFullTextError(null);
    try {
      const html = await invoke<string>("fetch_full_text", { id: articleId });
      setFullText(html);
      setArticles(prev => prev.map(a => a.id === articleId ? { ...a, full_content: html } : a));
    } catch (e) {
      setFullTextError(`${e}`);
    } finally {
      setFullTextLoading(false);
    }
  };

  // Removed prefetchNearby: pre-fetching adjacent articles pollutes
  // the full_content behavioral signal (full_content IS NOT NULL = user
  // explicitly chose to read) and wastes HTTP requests.

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
      if (e.key === "Home") {
        e.preventDefault();
        const items = getVisibleTreeItems();
        if (items.length > 0) { setSelectedFeedIndex(0); items[0].focus(); }
      }
      else if (e.key === "End") {
        e.preventDefault();
        const items = getVisibleTreeItems();
        if (items.length > 0) { const last = items.length - 1; setSelectedFeedIndex(last); items[last].focus(); }
      }
      else if (e.key === "ArrowDown") { e.preventDefault(); navigateFeed(1); }
      else if (e.key === "ArrowUp") { e.preventDefault(); navigateFeed(-1); }
      else if (e.key === "ArrowRight") {
        e.preventDefault();
        const el = document.activeElement as HTMLElement;
        const expanded = el?.getAttribute("aria-expanded");
        if (expanded === "false") {
          const sectionId = el?.dataset.section;
          if (sectionId) toggleSection(sectionId);
        } else if (expanded === "true") {
          navigateFeed(1);
        }
      }
      else if (e.key === "ArrowLeft") {
        e.preventDefault();
        const el = document.activeElement as HTMLElement;
        const expanded = el?.getAttribute("aria-expanded");
        if (expanded === "true") {
          // On expanded parent: collapse it
          const sectionId = el?.dataset.section;
          if (sectionId) toggleSection(sectionId);
        } else {
          // On child or collapsed parent: move focus to parent section header
          const group = el?.closest('[role="group"]');
          if (group) {
            const header = getSectionHeader(group as HTMLElement);
            if (header) {
              const items = getVisibleTreeItems();
              const idx = items.indexOf(header);
              if (idx >= 0) {
                setSelectedFeedIndex(idx);
                header.focus();
              }
            }
          }
        }
      }
      else if (e.key === "Enter") {
        e.preventDefault();
        const el = document.activeElement as HTMLElement;
        if (el?.getAttribute("aria-expanded") !== null && el?.dataset.section) {
          toggleSection(el.dataset.section);
          return;
        }
        const feedId = el?.dataset.feedId;
        const folderName = el?.dataset.folderName;
        if (el?.id === "feed-all") {
          loadAllArticles();
        } else if (folderName) {
          selectCognitiveFolder(folderName);
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
        e.preventDefault();
        const el = document.activeElement as HTMLElement;
        if (el) {
          const rect = el.getBoundingClientRect();
          const feedId = el.dataset.feedId;
          const folderId = el.dataset.folderId;
          if (feedId) {
            openContextMenu(rect.left + 20, rect.bottom, "feed", parseInt(feedId));
          } else if (folderId) {
            const folder = folders().find(f => f.id === parseInt(folderId));
            if (folder && folder.type === "manual") {
              openContextMenu(rect.left + 20, rect.bottom, "folder", parseInt(folderId));
            }
          }
        }
      }
    }

    if (pane === "articles") {
      if (e.key === "j" || e.key === "ArrowDown") { e.preventDefault(); navigateArticle(1); }
      else if (e.key === "k" || e.key === "ArrowUp") { e.preventDefault(); navigateArticle(-1); }
      else if (e.key === "PageDown") { e.preventDefault(); navigateArticle(10); }
      else if (e.key === "PageUp") { e.preventDefault(); navigateArticle(-10); }
      else if (e.key === "Home") {
        e.preventDefault();
        setSelectedArticleIndex(0);
        focusArticleItem(0);
      }
      else if (e.key === "End") {
        e.preventDefault();
        const last = sortedArticles().length - 1;
        if (last >= 0) { setSelectedArticleIndex(last); focusArticleItem(last); }
      }
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

    const handleClickOutside = (e: MouseEvent) => {
      if (contextMenu() && !(e.target as HTMLElement)?.closest('[role="menu"]')) {
        closeContextMenu();
      }
    };
    document.addEventListener("click", handleClickOutside);

    // Listen for async fetch completion events
    let unlistenFetch: (() => void) | null = null;
    listen<{ ok: boolean; results?: Array<{ feed_id: number; title: string; new_articles: number; error?: string }>; error?: string }>("fetch-complete", async (event) => {
      const payload = event.payload;
      if (payload.ok && payload.results) {
        const results = payload.results;
        const statusText = results.map(r => r.error ? `${r.title}: ${r.error}` : `${r.title}: ${r.new_articles} new`).join("; ");
        setStatus(statusText);
        setLastRefreshTime(Date.now());
        updateRefreshLabel();
        const fid = selectedFeed();
        await loadArticles(fid ?? undefined);
        await loadFolders();
      } else {
        setStatus(`Fetch error: ${payload.error || "Unknown error"}`);
      }
      setFeedBusy(false);
    }).then(fn => { unlistenFetch = fn; });

    const autoRefreshInterval = setInterval(() => {
      fetchAll();
    }, 15 * 60 * 1000);

    const labelUpdateInterval = setInterval(() => {
      updateRefreshLabel();
    }, 30 * 1000);

    onCleanup(() => {
      document.removeEventListener("keydown", handleKeyDown);
      document.removeEventListener("click", handleClickOutside);
      if (unlistenFetch) unlistenFetch();
      clearInterval(autoRefreshInterval);
      clearInterval(labelUpdateInterval);
    });
  });

  return (
    <div class="app" aria-label="RSS Reader">
      <h1 class="sr-only">RSS Reader</h1>

      <a href="#reader-pane" class="skip-link">Skip to main content</a>

      <div class="sr-only" aria-live="assertive" aria-atomic="true">
        {articleAnnouncement()}
      </div>

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
              <button
                onClick={() => {
                  setManageMode(!manageMode());
                  setSelectedFeedIds(new Set<number>());
                }}
                aria-label={manageMode() ? "Exit manage mode" : "Manage feeds"}
                aria-pressed={manageMode()}
              >
                {manageMode() ? "Done" : "Manage"}
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

          {/* New folder dialog */}
          <Show when={showNewFolder()}>
            <div class="feed-management" aria-label="Create folder">
              <form
                class="add-feed-form"
                onSubmit={(e) => {
                  e.preventDefault();
                  createNewFolder(newFolderName(), pendingMoveFeedId() ?? undefined);
                }}
                aria-label="Create new folder"
              >
                <label for="new-folder-name" class="sr-only">Folder name</label>
                <input
                  id="new-folder-name"
                  type="text"
                  placeholder="Folder name, then Enter"
                  value={newFolderName()}
                  onInput={(e) => setNewFolderName(e.currentTarget.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Escape") {
                      setShowNewFolder(false);
                      setNewFolderName("");
                      setPendingMoveFeedId(null);
                    }
                  }}
                />
                <button type="submit" aria-label="Create folder">OK</button>
                <button type="button" onClick={() => { setShowNewFolder(false); setNewFolderName(""); setPendingMoveFeedId(null); }} aria-label="Cancel">
                  Cancel
                </button>
              </form>
            </div>
          </Show>

          {/* Manage mode: bulk select + move */}
          <Show when={manageMode()}>
            <div class="manage-bar" role="toolbar" aria-label="Bulk feed actions">
              <span class="manage-count">{selectedFeedIds().size} selected</span>
              <select
                aria-label="Move selected feeds to folder"
                value={bulkTargetFolder() ?? ""}
                onChange={(e) => {
                  const val = e.currentTarget.value;
                  if (val === "__uncategorize") {
                    setBulkTargetFolder(-1);
                  } else {
                    setBulkTargetFolder(val ? parseInt(val) : null);
                  }
                }}
              >
                <option value="">Move to...</option>
                <For each={manualFolders()}>
                  {(f) => <option value={f.id ?? ""}>{f.name}</option>}
                </For>
                <option value="__uncategorize">Uncategorize</option>
              </select>
              <button
                disabled={selectedFeedIds().size === 0}
                onClick={() => {
                  const target = bulkTargetFolder();
                  if (target === -1) {
                    bulkMoveFeeds(null);
                  } else if (target) {
                    bulkMoveFeeds(target);
                  }
                }}
                aria-label="Move selected feeds"
              >
                Move
              </button>
              <button
                onClick={() => {
                  setPendingMoveFeedId(null);
                  setShowNewFolder(true);
                  requestAnimationFrame(() => document.getElementById("new-folder-name")?.focus());
                }}
                aria-label="Create new folder"
              >
                + Folder
              </button>
            </div>
          </Show>

          <ul
            ref={feedListRef}
            role="tree"
            aria-label="Feed subscriptions"
            tabindex={-1}
          >
            {/* All Articles */}
            <li
              id="feed-all"
              role="treeitem"
              aria-level={1}
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

            {/* Smart Folders section (4 fixed cognitive folders) */}
            <li
              role="treeitem"
              aria-level={1}
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
            <li role="none">
              <ul role="group" style={isSectionExpanded("smart-folders") ? {} : { display: "none" }}>
                <For each={cognitiveFolders()}>
                  {(folder) => (
                    <li
                      role="treeitem"
                      aria-level={2}
                      class="feed-button tree-child"
                      tabindex={-1}
                      aria-selected={selectedFolder() === folder.name}
                      aria-label={`${folder.name} smart folder, ${folder.article_count ?? 0} articles`}
                      data-folder-name={folder.name}
                      onClick={() => selectCognitiveFolder(folder.name)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter" || e.key === " ") {
                          e.preventDefault();
                          selectCognitiveFolder(folder.name);
                        }
                      }}
                    >
                      <span class="folder-icon" aria-hidden="true">{"\uD83D\uDCC1"}</span>
                      <span class="feed-title">{folder.name}</span>
                      <Show when={(folder.article_count ?? 0) > 0}>
                        <span class="unread-badge" aria-hidden="true">{folder.article_count}</span>
                      </Show>
                    </li>
                  )}
                </For>
              </ul>
            </li>

            {/* Manual Folders — each folder expands to show its feeds */}
            <For each={manualFolders()}>
              {(folder) => {
                const folderId = folder.id!;
                const sectionKey = `folder-${folderId}`;
                const folderFeeds = () => feedsInFolder(folderId);
                return (
                  <>
                    <li
                      role="treeitem"
                      aria-level={1}
                      aria-expanded={isSectionExpanded(sectionKey)}
                      data-section={sectionKey}
                      data-folder-id={folderId}
                      class="section-header feed-button"
                      tabindex={-1}
                      onClick={() => toggleSection(sectionKey)}
                      onContextMenu={(e) => {
                        e.preventDefault();
                        openContextMenu(e.clientX, e.clientY, "folder", folderId);
                      }}
                    >
                      <span class="section-toggle" aria-hidden="true">
                        {isSectionExpanded(sectionKey) ? "\u25BE" : "\u25B8"}
                      </span>
                      <span class="folder-icon" aria-hidden="true">{"\uD83D\uDCC2"}</span>
                      <span class="feed-title">{folder.name}</span>
                      <Show when={folderFeeds().length > 0}>
                        <span class="unread-badge" aria-hidden="true">{folderFeeds().length}</span>
                      </Show>
                    </li>
                    <li role="none">
                      <ul role="group" style={isSectionExpanded(sectionKey) ? {} : { display: "none" }}>
                        <For each={folderFeeds()}>
                          {(feed) => {
                            const feedUnread = () => articles().filter((a) => a.feed_id === feed.id && !a.is_read).length;
                            return (
                              <li
                                id={`feed-${feed.id}`}
                                role="treeitem"
                                aria-level={2}
                                class={`feed-button tree-child ${manageMode() && selectedFeedIds().has(feed.id) ? "manage-selected" : ""}`}
                                tabindex={-1}
                                aria-selected={manageMode() ? selectedFeedIds().has(feed.id) : selectedFeed() === feed.id}
                                aria-label={`${feed.title}${feedUnread() > 0 ? `, ${feedUnread()} unread` : ""}`}
                                data-feed-id={feed.id}
                                onClick={() => {
                                  if (manageMode()) {
                                    toggleFeedSelection(feed.id);
                                  } else {
                                    selectFeed(feed);
                                  }
                                }}
                                onContextMenu={(e) => {
                                  e.preventDefault();
                                  openContextMenu(e.clientX, e.clientY, "feed", feed.id);
                                }}
                                onKeyDown={(e) => {
                                  if (manageMode() && (e.key === "Enter" || e.key === " ")) {
                                    e.preventDefault();
                                    toggleFeedSelection(feed.id);
                                    return;
                                  }
                                  if (e.key === "Delete") {
                                    removeFeed(feed.id);
                                  }
                                  if (e.key === "Enter" || e.key === " ") {
                                    e.preventDefault();
                                    selectFeed(feed);
                                  }
                                }}
                              >
                                <Show when={manageMode()}>
                                  <span class="manage-checkbox" aria-hidden="true">
                                    {selectedFeedIds().has(feed.id) ? "\u2611" : "\u2610"}
                                  </span>
                                </Show>
                                <span class="feed-title">{feed.title}</span>
                                <Show when={feedUnread() > 0 && !manageMode()}>
                                  <span class="unread-badge" aria-hidden="true">{feedUnread()}</span>
                                </Show>
                              </li>
                            );
                          }}
                        </For>
                      </ul>
                    </li>
                  </>
                );
              }}
            </For>

            {/* Feeds section */}
            <li
              role="treeitem"
              aria-level={1}
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
            <li role="none">
              <ul role="group" style={isSectionExpanded("feeds") ? {} : { display: "none" }}>
                <For each={uncategorizedFeeds()}>
                  {(feed) => {
                    const feedUnread = () => articles().filter((a) => a.feed_id === feed.id && !a.is_read).length;
                    return (
                      <li
                        id={`feed-${feed.id}`}
                        role="treeitem"
                        aria-level={2}
                        class={`feed-button tree-child ${manageMode() && selectedFeedIds().has(feed.id) ? "manage-selected" : ""}`}
                        tabindex={-1}
                        aria-selected={manageMode() ? selectedFeedIds().has(feed.id) : selectedFeed() === feed.id}
                        aria-label={`${feed.title}${feedUnread() > 0 ? `, ${feedUnread()} unread` : ""}${manageMode() ? (selectedFeedIds().has(feed.id) ? ", selected" : ", not selected") : ""}`}
                        data-feed-id={feed.id}
                        onClick={() => {
                          if (manageMode()) {
                            toggleFeedSelection(feed.id);
                          } else {
                            selectFeed(feed);
                          }
                        }}
                        onContextMenu={(e) => {
                          e.preventDefault();
                          openContextMenu(e.clientX, e.clientY, "feed", feed.id);
                        }}
                        onKeyDown={(e) => {
                          if (manageMode() && (e.key === "Enter" || e.key === " ")) {
                            e.preventDefault();
                            toggleFeedSelection(feed.id);
                            return;
                          }
                          if (e.key === "Delete") {
                            removeFeed(feed.id);
                          }
                          if (e.key === "Enter" || e.key === " ") {
                            e.preventDefault();
                            selectFeed(feed);
                          }
                        }}
                      >
                        <Show when={manageMode()}>
                          <span class="manage-checkbox" aria-hidden="true">
                            {selectedFeedIds().has(feed.id) ? "\u2611" : "\u2610"}
                          </span>
                        </Show>
                        <span class="feed-title">{feed.title}</span>
                        <Show when={feedUnread() > 0 && !manageMode()}>
                          <span class="unread-badge" aria-hidden="true">{feedUnread()}</span>
                        </Show>
                      </li>
                    );
                  }}
                </For>
              </ul>
            </li>
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
          aria-label="Article list"
        >
          <div class="pane-header">
            <h2>Articles</h2>
            <div class="pane-actions">
              <button
                onClick={() => setSortOrder(sortOrder() === "newest" ? "oldest" : "newest")}
                aria-label={`Sort by ${sortOrder() === "newest" ? "oldest first" : "newest first"}`}
              >
                {sortOrder() === "newest" ? "\u2193 New" : "\u2191 Old"}
              </button>
            </div>
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
                clearTimeout(searchTimer);
                searchTimer = setTimeout(() => searchArticles(q), 300);
              }}
            />
          </div>
          <div
            ref={articleListRef}
            role="feed"
            aria-label="Articles"
            aria-busy={feedBusy()}
          >
            <Show when={feedBusy() && articles().length === 0}>
              <div class="empty-state" aria-label="Loading articles">
                <p>Loading articles...</p>
              </div>
            </Show>
            <Show when={!feedBusy() && articles().length === 0}>
              <div class="empty-state">
                <p>No articles to display</p>
              </div>
            </Show>
            <For each={sortedArticles()}>
              {(article, index) => {
                const titleId = `article-title-${article.id}`;
                const snippetId = `article-snippet-${article.id}`;
                const dateLabel = () => formatDate(article.published_at || article.fetched_at);
                const readStatus = article.is_read ? "read" : "unread";
                const ariaDesc = `${readStatus}${article.is_starred ? ", starred" : ""}${dateLabel() ? `, ${dateLabel()}` : ""}`;
                return (
                  <article
                    id={`article-${article.id}`}
                    role="article"
                    aria-posinset={index() + 1}
                    aria-setsize={sortedArticles().length}
                    aria-label={`${article.title}, ${ariaDesc}`}
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
                    <Show when={dateLabel()}>
                      <span class="article-date" aria-hidden="true">{dateLabel()}</span>
                    </Show>
                    <Show when={article.summary}>
                      <span id={snippetId} class="article-summary">
                        {article.summary!.length > 120 ? article.summary!.substring(0, 117) + "..." : article.summary}
                      </span>
                    </Show>
                  </article>
                );
              }}
            </For>
            <Show when={hasMore()}>
              <button
                class="load-more-btn"
                onClick={loadMoreArticles}
                disabled={feedBusy()}
                aria-label="Load more articles"
              >
                {feedBusy() ? "Loading..." : "Load more articles"}
              </button>
            </Show>
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
              <div class="empty-state">
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
                <Show when={fullTextLoading()}>
                  <p class="article-content" style={{"color": "var(--read)"}}>Loading full text...</p>
                </Show>
                <Show when={!fullTextLoading()}>
                  <div
                    class="article-content"
                    innerHTML={fullText() || article().full_content || article().content || article().summary || ""}
                  />
                  <Show when={!(fullText() || article().full_content || article().content || article().summary)}>
                    <div class="article-content" style={{"color": "var(--read)"}}>
                      <p>No content in feed.{article().url ? "" : " No article URL available."}</p>
                      <Show when={article().url}>
                        <p>This may be a paywalled or members-only article. <a href={article().url!} target="_blank" rel="noopener noreferrer">Open in browser</a> to read.</p>
                      </Show>
                    </div>
                  </Show>
                </Show>
              </article>
            )}
          </Show>
        </main>
      </div>
    </div>
  );
}
