import "./styles.css";

const app = document.querySelector<HTMLDivElement>("#app");

if (!app) {
  throw new Error("MediaIndex root element was not found.");
}

app.innerHTML = `
  <main class="shell">
    <header class="topbar">
      <div>
        <p class="eyebrow">LOCAL-FIRST MEDIA INDEX</p>
        <h1>Find the right shot.</h1>
        <p class="lede">
          Start with a local folder. MediaIndex will keep the original footage
          where it already lives.
        </p>
      </div>
      <button class="primary-button" id="select-folder" type="button">
        Select footage folder
      </button>
    </header>

    <section class="workspace" aria-label="Media library">
      <aside class="sidebar">
        <p class="section-label">Library</p>
        <div class="library-card">
          <span class="status-dot" aria-hidden="true"></span>
          <div>
            <strong>No folder indexed</strong>
            <span>Choose a local folder to begin</span>
          </div>
        </div>
        <p class="section-label">Filters</p>
        <div class="filter-placeholder">
          <span>Metadata filters arrive with the local scanner.</span>
        </div>
      </aside>

      <section class="content-panel">
        <div class="panel-heading">
          <div>
            <p class="section-label">Local library</p>
            <h2>Ready when you are</h2>
          </div>
          <span class="count-badge">0 clips</span>
        </div>
        <div class="empty-state">
          <div class="empty-icon" aria-hidden="true">⌁</div>
          <h3>Your footage stays on your machine</h3>
          <p>
            The next step is the local scanner: discovery, FFprobe metadata,
            content hashing, and a searchable SQLite index.
          </p>
          <button class="secondary-button" id="learn-more" type="button">
            View the MVP plan
          </button>
        </div>
      </section>
    </section>
  </main>
`;

document.querySelector<HTMLButtonElement>("#select-folder")?.addEventListener(
  "click",
  () => {
    window.alert("Folder selection will be connected to Tauri in issue #3.");
  },
);

document.querySelector<HTMLButtonElement>("#learn-more")?.addEventListener(
  "click",
  () => {
    window.alert("See docs/project-plan.md for the current MVP scope.");
  },
);
