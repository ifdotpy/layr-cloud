import type { ComponentChildren } from "preact";
import { useEffect, useState } from "preact/hooks";
import type { DataSource, Project, You } from "./data/source";
import { parseRoute, projectUrl } from "./router";
import {
  Avatar,
  Badge,
  Cmd,
  Dialog,
  Icon,
  LoadError,
  ReadOnlyAction,
  useData,
} from "./components/ui";
import { CodePage, LinesPage, StatesPage, StatePage } from "./pages/code";
import { ChangesPage, ChangePage } from "./pages/changes";
import { ChecksPage, DeployPage, ActivityPage } from "./pages/operations";
import { OrgPage } from "./pages/org";
import { SettingsPage } from "./pages/settings";
import { OverlaysPage, OverlayPage } from "./pages/overlays";
import { loadChangeIndex } from "./data/change-index";

const PROJECT_TABS = [
  "Code",
  "Changes",
  "Checks",
  "Deploy",
  "Activity",
  "Settings",
];
const ORG_TABS: [string, string][] = [
  ["Projects", "projects"],
  ["People", "people"],
  ["Infrastructure", "infrastructure"],
  ["Settings", "org-settings"],
];
export function App({
  source,
  onLock,
}: {
  source: DataSource;
  onLock?: () => void;
}) {
  const [hash, setHash] = useState(location.hash);
  const [menu, setMenu] = useState("");
  const [cli, setCli] = useState(false);
  const [clone, setClone] = useState(false);
  const [cloneTab, setCloneTab] = useState("Clone");
  const [search, setSearch] = useState("");
  const route = parseRoute(hash);
  const { data: projects, error } = useData(() => source.projects(), [source]);
  const { data: you } = useData(() => source.you(), [source]);
  const project = projects?.find((p) => p.id === route.project);
  const { data: copies } = useData(
    async () => (project ? source.copies(project.id) : undefined),
    [source, project?.id],
  );
  const { data: changeIndex, error: changeCountError } = useData(
    async () => (project ? loadChangeIndex(source, project) : undefined),
    [source, project?.id],
  );
  useEffect(() => {
    const change = () => {
      setHash(location.hash);
      setMenu("");
      window.scrollTo(0, 0);
    };
    window.addEventListener("hashchange", change);
    return () => window.removeEventListener("hashchange", change);
  }, []);
  useEffect(() => {
    document.documentElement.classList.toggle("cli", cli);
  }, [cli]);
  useEffect(() => {
    const close = (e: MouseEvent) => {
      if (!(e.target as Element).closest(".menu-wrap")) setMenu("");
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") setMenu("");
      if (
        e.key === "/" &&
        !/INPUT|TEXTAREA|SELECT/.test(document.activeElement?.tagName ?? "")
      ) {
        e.preventDefault();
        document.querySelector<HTMLInputElement>(".search input")?.focus();
      }
    };
    document.addEventListener("click", close);
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("click", close);
      document.removeEventListener("keydown", key);
    };
  }, []);
  useEffect(() => {
    document.title = `${project?.name ?? "Layr Cloud"} · ${route.page === "org-settings" ? "Settings" : route.page[0]?.toUpperCase() + route.page.slice(1)} · Layr`;
  }, [project?.name, route.page]);
  if (!projects)
    return (
      <main class="page">
        <LoadError error={error} />
      </main>
    );
  const org = project?.organization ?? projects[0]?.organization ?? "Projects";
  const tab = ["lines", "states", "state", "overlays", "overlay"].includes(
    route.page,
  )
    ? "Code"
    : ["change", "changes"].includes(route.page)
      ? "Changes"
      : (PROJECT_TABS.find((t) => t.toLowerCase() === route.page) ?? "Code");
  return (
    <>
      <header>
        <div class="topbar">
          <a class="wordmark" href="#/projects">
            Layr
          </a>
          <nav class="crumbs" aria-label="Breadcrumb">
            <a href="#/projects">{org}</a>
            {project && (
              <>
                <span class="sep">/</span>
                <a href={projectUrl(project.id)}>{project.name}</a>
              </>
            )}
          </nav>
          <form
            class="search"
            role="search"
            onSubmit={(e) => {
              e.preventDefault();
              const match = projects.find((p) =>
                p.name.toLowerCase().includes(search.toLowerCase()),
              );
              if (match) location.hash = projectUrl(match.id).slice(1);
            }}
          >
            <Icon name="search" />
            <label class="sr" for="global-search">
              Search projects
            </label>
            <input
              id="global-search"
              type="search"
              placeholder="Search or jump to…"
              value={search}
              onInput={(e) => setSearch(e.currentTarget.value)}
            />
            <kbd>/</kbd>
          </form>
          <div class="right">
            <div class="menu-wrap">
              <button
                class="iconbtn"
                type="button"
                aria-label="Create"
                aria-haspopup="true"
                aria-expanded={menu === "create"}
                onClick={() => setMenu(menu === "create" ? "" : "create")}
              >
                +
              </button>
              <div class={`menu end ${menu === "create" ? "open" : ""}`}>
                <a href="#/projects">New project</a>
                {project && (
                  <>
                    <a href={projectUrl(project.id, "lines")}>New line</a>
                    <a href={projectUrl(project.id, "changes")}>New change</a>
                  </>
                )}
                <a href="#/people">Invite person</a>
              </div>
            </div>
            <div class="menu-wrap">
              <button
                class="iconbtn"
                type="button"
                aria-label={`Account${you?.login ? " of " + you.login : ""}`}
                aria-haspopup="true"
                aria-expanded={menu === "account"}
                onClick={() => setMenu(menu === "account" ? "" : "account")}
              >
                <Avatar name={you?.login ?? "?"} />
              </button>
              <div class={`menu end ${menu === "account" ? "open" : ""}`}>
                {you?.login && <div class="lbl">Signed in as {you.login}</div>}
                <a href="#/you">You: key, invites, disk</a>
                <button type="button" onClick={() => setCli(!cli)}>
                  {cli ? "Hide" : "Show"} CLI commands
                </button>
                <hr />
                <button
                  type="button"
                  onClick={async () => {
                    await source.lock();
                    onLock?.();
                  }}
                >
                  Lock
                </button>
              </div>
            </div>
          </div>
        </div>
        <div class="projhead">
          <h1>
            <a href="#/projects">{org}</a>
            {project && (
              <>
                {" "}
                <span class="muted">/</span>{" "}
                <a href={projectUrl(project.id)}>{project.name}</a>
              </>
            )}
          </h1>
          {project?.policy?.protect?.[project.default_line ?? "main"] && (
            <Badge>protected {project.default_line ?? "main"}</Badge>
          )}
          {project && (
            <div class="right">
              <button class="btn" type="button" onClick={() => setClone(true)}>
                Clone
              </button>
            </div>
          )}
        </div>
        <div class="tabs-wrap">
          <nav class="tabs" aria-label={project ? "Project" : "Organization"}>
            {project
              ? PROJECT_TABS.map((t) => (
                  <a
                    href={projectUrl(
                      project.id,
                      t === "Settings"
                        ? "settings/access"
                        : t === "Deploy"
                          ? "deploy/environments"
                          : t.toLowerCase(),
                    )}
                    aria-current={tab === t ? "page" : undefined}
                  >
                    {t}
                    {t === "Changes" && (
                      <span
                        class="count"
                        aria-label="Open changes"
                        aria-busy={!changeIndex && !changeCountError}
                        title={changeCountError?.message}
                      >
                        {changeIndex?.open ?? (changeCountError ? "?" : "…")}
                      </span>
                    )}
                  </a>
                ))
              : ORG_TABS.map(([name, path]) => (
                  <a
                    href={`#/${path}`}
                    aria-current={route.page === path ? "page" : undefined}
                  >
                    {name}
                    {name === "Projects" && (
                      <span class="count">{projects.length}</span>
                    )}
                  </a>
                ))}
          </nav>
        </div>
      </header>
      <main class="page" id="main-content" key={hash}>
        {route.project && !project ? (
          <div class="callout bad" role="alert">
            Project not found.
          </div>
        ) : project ? (
          <ProjectPage source={source} project={project} route={route} />
        ) : (
          <OrgPage source={source} page={route.page} />
        )}
      </main>
      {project && (
        <Dialog
          title={`Clone ${project.name}`}
          open={clone}
          onClose={() => setClone(false)}
        >
          <div class="dtabs" role="tablist" aria-label="Get project">
            {["Clone", "Mount", "Git"].map((t) => (
              <button
                role="tab"
                type="button"
                aria-selected={t === cloneTab}
                aria-controls={`clone-${t}`}
                id={`tab-${t}`}
                onClick={() => setCloneTab(t)}
              >
                {t}
              </button>
            ))}
          </div>
          {["Clone", "Mount", "Git"].map((t) => (
            <div
              role="tabpanel"
              id={`clone-${t}`}
              aria-labelledby={`tab-${t}`}
              hidden={t !== cloneTab}
            >
              <p class="muted">
                {t === "Clone"
                  ? "Get the project with all lines and states. Data is decrypted on your machine."
                  : t === "Mount"
                    ? "Opens the project as a folder, read only, decrypted on your machine."
                    : "Mirror states to a Git host when you need one."}
              </p>
              <Cmd
                text={
                  t === "Clone"
                    ? `layr clone ${copies?.find((c) => c.kind === "store")?.address ?? "<store-address>"}`
                    : t === "Mount"
                      ? `layr mount ${copies?.find((c) => c.kind === "store")?.name ?? "<store>"} ~/${project.name}`
                      : "layr git push github main"
                }
              />
            </div>
          ))}
        </Dialog>
      )}
    </>
  );
}
function ProjectPage({
  source,
  project,
  route,
}: {
  source: DataSource;
  project: Project;
  route: ReturnType<typeof parseRoute>;
}) {
  switch (route.page) {
    case "code":
      return <CodePage source={source} project={project} route={route} />;
    case "overlays":
      return <OverlaysPage source={source} project={project} />;
    case "overlay":
      return (
        <OverlayPage
          source={source}
          project={project}
          id={route.section ?? ""}
        />
      );
    case "lines":
      return <LinesPage source={source} project={project} />;
    case "states":
      return <StatesPage source={source} project={project} route={route} />;
    case "state":
      return (
        <StatePage
          source={source}
          project={project}
          stateId={route.section ?? ""}
        />
      );
    case "changes":
      return (
        <ChangesPage
          source={source}
          project={project}
          section={route.section ?? "open"}
        />
      );
    case "change":
      return (
        <ChangePage
          source={source}
          project={project}
          id={route.section ?? ""}
          section={route.id ?? "overview"}
        />
      );
    case "checks":
      return (
        <ChecksPage
          source={source}
          project={project}
          behavior={route.section === "behavior"}
        />
      );
    case "deploy":
      return (
        <DeployPage
          source={source}
          project={project}
          section={route.section ?? "environments"}
          reference={route.id}
        />
      );
    case "activity":
      return <ActivityPage source={source} project={project} />;
    case "settings":
      return (
        <SettingsPage
          source={source}
          project={project}
          page={route.section ?? "access"}
        />
      );
    default:
      return (
        <div class="callout bad" role="alert">
          Page not found.
        </div>
      );
  }
}
