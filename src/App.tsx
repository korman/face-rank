import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  CheckCircle2,
  FolderOpen,
  Image as ImageIcon,
  ListOrdered,
  RefreshCw,
  SkipForward,
  Undo2,
} from "lucide-react";
import "./App.css";

type View = "vote" | "rankings";

type AppSummary = {
  photoDirectory: string | null;
  activePhotoCount: number;
  comparisonCount: number;
};

type PhotoSummary = {
  id: number;
  fileName: string;
  thumbnailPath: string;
  mu: number;
  sigma: number;
};

type ComparisonPair = {
  left: PhotoSummary;
  right: PhotoSummary;
};

type ScanReport = AppSummary & {
  addedCount: number;
  invalidCount: number;
};

type RankingRow = {
  rank: number;
  id: number;
  fileName: string;
  thumbnailPath: string;
  ordinal: number;
  mu: number;
  sigma: number;
  comparisonCount: number;
  wins: number;
  losses: number;
  winRate: number;
  sampleSmall: boolean;
};

function App() {
  const initialized = useRef(false);
  const busyRef = useRef(false);
  const [view, setView] = useState<View>("vote");
  const [summary, setSummary] = useState<AppSummary>({
    photoDirectory: null,
    activePhotoCount: 0,
    comparisonCount: 0,
  });
  const [pair, setPair] = useState<ComparisonPair | null>(null);
  const [rankings, setRankings] = useState<RankingRow[]>([]);
  const [busy, setBusy] = useState(false);
  const [busyMessage, setBusyMessage] = useState("");
  const [initializing, setInitializing] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const refreshSummary = useCallback(async () => {
    const nextSummary = await invoke<AppSummary>("get_app_state");
    setSummary(nextSummary);
    return nextSummary;
  }, []);

  const refreshRankings = useCallback(async () => {
    setRankings(await invoke<RankingRow[]>("get_rankings"));
  }, []);

  const loadNextPair = useCallback(async () => {
    setPair(await invoke<ComparisonPair | null>("get_next_comparison"));
  }, []);

  const refreshAll = useCallback(async () => {
    const nextSummary = await refreshSummary();
    await refreshRankings();
    if (nextSummary.activePhotoCount >= 2) {
      await loadNextPair();
    } else {
      setPair(null);
    }
  }, [loadNextPair, refreshRankings, refreshSummary]);

  useEffect(() => {
    if (initialized.current) return;
    initialized.current = true;
    void (async () => {
      try {
        const nextSummary = await refreshSummary();
        await refreshRankings();
        if (nextSummary.photoDirectory) {
          await invoke<ScanReport>("rescan_photo_directory");
          await refreshAll();
        }
      } catch (cause) {
        setError(formatError(cause));
      } finally {
        setInitializing(false);
      }
    })();
  }, [refreshAll, refreshRankings, refreshSummary]);

  const runAction = useCallback(async (message: string, action: () => Promise<void>) => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setBusyMessage(message);
    setError(null);
    setNotice(null);
    try {
      await action();
    } catch (cause) {
      setError(formatError(cause));
      try {
        await refreshAll();
      } catch {
        // Preserve the original error; a later action can retry the refresh.
      }
    } finally {
      busyRef.current = false;
      setBusy(false);
      setBusyMessage("");
    }
  }, [refreshAll]);

  const chooseDirectory = useCallback(async () => {
    await runAction("正在读取照片目录", async () => {
      const selected = await open({ directory: true, multiple: false, title: "选择照片文件夹" });
      if (!selected || Array.isArray(selected)) return;
      setPair(null);
      const report = await invoke<ScanReport>("set_photo_directory", { path: selected });
      setSummary(report);
      await refreshRankings();
      await loadNextPair();
      setNotice(
        report.invalidCount > 0
          ? `已读取 ${report.activePhotoCount} 张照片，跳过 ${report.invalidCount} 个无效文件`
          : `已读取 ${report.activePhotoCount} 张照片`,
      );
    });
  }, [loadNextPair, refreshRankings, runAction]);

  const rescan = useCallback(async () => {
    await runAction("正在重新扫描照片目录", async () => {
      setPair(null);
      const report = await invoke<ScanReport>("rescan_photo_directory");
      setSummary(report);
      await refreshRankings();
      await loadNextPair();
      setNotice(
        `扫描完成：${report.activePhotoCount} 张有效照片，新增 ${report.addedCount} 张，跳过 ${report.invalidCount} 个无效文件`,
      );
    });
  }, [loadNextPair, refreshRankings, runAction]);

  const submitVote = useCallback(
    async (winnerId: number) => {
      await runAction("正在保存比较结果", async () => {
        setPair(null);
        await invoke("record_comparison", { winnerId });
        await refreshAll();
      });
    },
    [refreshAll, runAction],
  );

  const skipPair = useCallback(async () => {
    await runAction("正在选择下一组照片", async () => {
      await loadNextPair();
      setNotice("已跳过这一组比较");
    });
  }, [loadNextPair, runAction]);

  const undo = useCallback(async () => {
    await runAction("正在撤销比较", async () => {
      setPair(null);
      const undone = await invoke<boolean>("undo_last_comparison");
      await refreshAll();
      setNotice(undone ? "已撤销最近一次比较" : "还没有可撤销的比较");
    });
  }, [refreshAll, runAction]);

  const directoryLabel = useMemo(() => {
    if (!summary.photoDirectory) return "尚未选择照片文件夹";
    const segments = summary.photoDirectory.split(/[\\/]/).filter(Boolean);
    return segments[segments.length - 1] || summary.photoDirectory;
  }, [summary.photoDirectory]);

  return (
    <div className="app-shell">
      <header className="topbar">
        <div className="brand-lockup">
          <span className="brand-mark">FR</span>
          <div>
            <p className="eyebrow">LOCAL PHOTO RANKING</p>
            <h1>Face Rank</h1>
          </div>
        </div>
        <div className="topbar-actions">
          <span className={`collection-status ${summary.photoDirectory ? "has-directory" : ""}`} title={summary.photoDirectory || "尚未选择照片文件夹"}>
            <span className="status-dot" />
            {directoryLabel}
          </span>
          <button className="button button-quiet" type="button" onClick={() => void chooseDirectory()} disabled={busy}>
            <FolderOpen size={15} strokeWidth={1.8} /> 选择文件夹
          </button>
        </div>
      </header>

      <div className="workspace">
        <nav className="sidebar" aria-label="主导航">
          <div className="nav-group">
            <p className="nav-label">工作区</p>
            <button className={`nav-item ${view === "vote" ? "active" : ""}`} type="button" aria-current={view === "vote" ? "page" : undefined} onClick={() => setView("vote")}>
              <span className="nav-icon"><ImageIcon size={16} strokeWidth={1.8} /></span>
              评选
            </button>
            <button className={`nav-item ${view === "rankings" ? "active" : ""}`} type="button" aria-current={view === "rankings" ? "page" : undefined} onClick={() => setView("rankings")}>
              <span className="nav-icon"><ListOrdered size={16} strokeWidth={1.8} /></span>
              排行
            </button>
          </div>
          <div className="sidebar-footer">
            <div className="stat-line"><span>照片</span><strong>{summary.activePhotoCount}</strong></div>
            <div className="stat-line"><span>比较</span><strong>{summary.comparisonCount}</strong></div>
            <button className="text-button" type="button" onClick={() => void rescan()} disabled={!summary.photoDirectory || busy}>
              <span><RefreshCw size={14} strokeWidth={1.8} /></span> 重新扫描
            </button>
          </div>
        </nav>

        <main className="main-content" aria-busy={busy}>
          {error && <div className="alert alert-error" role="alert">{error}</div>}
          {notice && <div className="alert alert-notice" role="status">{notice}</div>}
          {busy && <div className="alert alert-loading" role="status"><span className="loading-indicator small" />{busyMessage}</div>}
          {initializing ? (
            <div className="empty-panel compact-empty"><span className="loading-indicator" />正在准备照片库</div>
          ) : view === "vote" ? (
            <VoteView
              pair={pair}
              busy={busy}
              activePhotoCount={summary.activePhotoCount}
              onVote={submitVote}
              onSkip={skipPair}
              onUndo={undo}
              onChooseDirectory={chooseDirectory}
              hasDirectory={Boolean(summary.photoDirectory)}
            />
          ) : (
            <RankingView rankings={rankings} onChooseDirectory={chooseDirectory} hasDirectory={Boolean(summary.photoDirectory)} />
          )}
        </main>
      </div>
    </div>
  );
}

function VoteView({
  pair,
  busy,
  activePhotoCount,
  onVote,
  onSkip,
  onUndo,
  onChooseDirectory,
  hasDirectory,
}: {
  pair: ComparisonPair | null;
  busy: boolean;
  activePhotoCount: number;
  onVote: (winnerId: number) => Promise<void>;
  onSkip: () => Promise<void>;
  onUndo: () => Promise<void>;
  onChooseDirectory: () => Promise<void>;
  hasDirectory: boolean;
}) {
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (
        event.repeat ||
        event.ctrlKey ||
        event.metaKey ||
        event.altKey ||
        busy ||
        !pair ||
        isInteractiveTarget(event.target)
      ) return;
      if (event.key.toLowerCase() === "a") {
        event.preventDefault();
        void onVote(pair.left.id);
      } else if (event.key.toLowerCase() === "d") {
        event.preventDefault();
        void onVote(pair.right.id);
      } else if (event.code === "Space") {
        event.preventDefault();
        void onSkip();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [busy, onSkip, onVote, pair]);

  if (!hasDirectory) {
    return <EmptyState onChooseDirectory={onChooseDirectory} title="从一组照片开始" description="选择一个文件夹，Face Rank 会递归读取其中的 PNG、JPG 和 JPEG 文件。" />;
  }

  return (
    <section className="vote-page">
      <div className="page-heading">
        <div>
          <p className="eyebrow">CURRENT MATCH</p>
          <h2>哪一张更好？</h2>
          <p className="muted">选择更符合你偏好的照片，评分会随着每次比较更新。</p>
        </div>
        <div className="heading-actions">
          <button className="button button-quiet" type="button" onClick={() => void onUndo()} disabled={busy}><Undo2 size={14} strokeWidth={1.8} /> 撤销</button>
          <button className="button button-quiet" type="button" onClick={() => void onSkip()} disabled={busy || !pair}><SkipForward size={14} strokeWidth={1.8} /> 跳过</button>
        </div>
      </div>
      {pair ? (
        <div className="comparison-stage">
          <PhotoChoice photo={pair.left} side="A" disabled={busy} onChoose={onVote} />
          <div className="versus" aria-hidden="true"><span>VS</span></div>
          <PhotoChoice photo={pair.right} side="D" disabled={busy} onChoose={onVote} />
        </div>
      ) : (
        <div className="empty-panel compact-empty">
          <span className="empty-symbol"><CheckCircle2 size={20} strokeWidth={1.7} /></span>
          {activePhotoCount < 2 ? (
            <>
              <h3>还需要更多照片</h3>
              <p>当前读取到 {activePhotoCount} 张有效照片，至少需要两张照片才能开始评选。</p>
            </>
          ) : (
            <>
              <h3>暂时无法载入下一组</h3>
              <p>请重新扫描照片目录或稍后重试。</p>
            </>
          )}
        </div>
      )}
    </section>
  );
}

function PhotoChoice({ photo, side, disabled, onChoose }: { photo: PhotoSummary; side: string; disabled: boolean; onChoose: (id: number) => Promise<void> }) {
  return (
    <button className="photo-choice" type="button" disabled={disabled} aria-label={`选择 ${photo.fileName}`} aria-keyshortcuts={side} onClick={() => void onChoose(photo.id)}>
      <div className="photo-frame">
        <img src={convertFileSrc(photo.thumbnailPath)} alt="" />
        <span className="side-tag">{side}</span>
        <span className="choose-hint">选择这张</span>
      </div>
      <span className="photo-name" title={photo.fileName}>{photo.fileName}</span>
    </button>
  );
}

function RankingView({ rankings, onChooseDirectory, hasDirectory }: { rankings: RankingRow[]; onChooseDirectory: () => Promise<void>; hasDirectory: boolean }) {
  return (
    <section className="ranking-page">
      <div className="page-heading">
        <div>
          <p className="eyebrow">RANKING BOARD</p>
          <h2>当前排行</h2>
          <p className="muted">按稳健分排序：μ − 3σ。样本越多，不确定度越低。</p>
        </div>
        <span className="ranking-count">{rankings.length} 张照片</span>
      </div>
      {!hasDirectory || rankings.length === 0 ? (
        <EmptyState onChooseDirectory={onChooseDirectory} title="排行还为空" description="选择照片文件夹后，完成几次比较，这里会显示当前结果。" />
      ) : (
        <div className="ranking-table-wrap">
          <table className="ranking-table">
            <thead><tr><th scope="col">排名</th><th scope="col">照片</th><th scope="col">稳健分</th><th scope="col">μ</th><th scope="col">σ</th><th scope="col">比较</th><th scope="col">胜率</th></tr></thead>
            <tbody>{rankings.map((row) => <RankingRowView key={row.id} row={row} />)}</tbody>
          </table>
        </div>
      )}
    </section>
  );
}

function RankingRowView({ row }: { row: RankingRow }) {
  return (
    <tr>
      <td><span className={`rank-number ${row.rank <= 3 ? "top-rank" : ""}`}>{String(row.rank).padStart(2, "0")}</span></td>
      <td><div className="ranking-photo"><img src={convertFileSrc(row.thumbnailPath)} alt="" loading="lazy" decoding="async" /><span title={row.fileName}>{row.fileName}</span></div></td>
      <td><strong>{row.ordinal.toFixed(1)}</strong></td>
      <td>{row.mu.toFixed(1)}</td>
      <td>{row.sigma.toFixed(1)}</td>
      <td>{row.comparisonCount}{row.sampleSmall && <span className="sample-badge">样本少</span>}</td>
      <td><span className="win-rate">{(row.winRate * 100).toFixed(0)}%</span><span className="record">{row.wins}胜 / {row.losses}负</span></td>
    </tr>
  );
}

function EmptyState({ onChooseDirectory, title, description }: { onChooseDirectory: () => Promise<void>; title: string; description: string }) {
  return <div className="empty-panel"><span className="empty-symbol"><FolderOpen size={20} strokeWidth={1.7} /></span><h3>{title}</h3><p>{description}</p><button className="button button-primary" type="button" onClick={() => void onChooseDirectory()}><FolderOpen size={15} strokeWidth={1.8} /> 选择照片文件夹</button></div>;
}

function formatError(cause: unknown): string {
  if (typeof cause === "string") return cause;
  if (cause && typeof cause === "object" && "message" in cause) return String(cause.message);
  return "操作失败，请重试";
}

function isInteractiveTarget(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  if (target instanceof HTMLElement && target.isContentEditable) return true;
  return Boolean(target.closest(
    "a[href], area[href], button, input, select, textarea, summary, [contenteditable]:not([contenteditable='false']), [role='button'], [role='link'], [role='textbox'], [role='combobox'], [role='checkbox'], [role='radio'], [role='switch'], [role='menuitem'], [tabindex]:not([tabindex='-1'])",
  ));
}

export default App;
