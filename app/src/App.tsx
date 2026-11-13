import { createEffect, createMemo, createResource, createSignal, For, on, onCleanup, Show } from "solid-js";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { listen } from "@tauri-apps/api/event";
import { createVirtualizer } from "@tanstack/solid-virtual";
import { commands, type Commit, type RepoOverview } from "./bindings";
import { Icon, type IconName } from "./icons";
import "./App.css";

const name = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;
// Nombre a mostrar: el alias del usuario; si no, el del repo en GitHub (origin); si no, el de la carpeta.
const repoLabel = (r: { path: string; alias: string; repo?: string | null }) => r.alias || r.repo || name(r.path);
const split = (p: string) => {
  const i = p.lastIndexOf("/");
  return [p.slice(0, i + 1), p.slice(i + 1)];
};

const statusIcon: Record<string, "diffAdded" | "diffModified" | "diffRemoved" | "diffRenamed"> = {
  Added: "diffAdded", Modified: "diffModified", Removed: "diffRemoved", TypeChange: "diffModified", Rename: "diffRenamed",
};
const isMac = navigator.userAgent.includes("Mac");

// Fotos de GitHub: opt-in (por defecto apagado). Al activarlas, el webview pide la imagen a
// github.com/avatars con el id/login del correo `noreply` de GitHub del commit: nunca el correo real
// ni su hash (Gravatar). Sin foto, se muestran iniciales.
// ¿hay sesión de GitHub? Con sesión se muestran fotos; sin ella, iniciales.
// Errores de git por falta/rechazo de credenciales HTTPS (git corre sin terminal: no puede pedirlas).
const OPS: Record<string, string> = { merge: "Fusión", "cherry-pick": "Cherry-pick", revert: "Revert", rebase: "Rebase" };
// Ayuda: lista de temas a la izquierda y detalle a la derecha.
type Topic = { id: string; icon: IconName; title: string; group: string; what: string; when: string; how: string; note?: string };
const TOPICS: Topic[] = [
  { id: "fetch", icon: "sync", group: "Sincronizar", title: "Fetch", what: "Pregunta a GitHub qué hay de nuevo y descarga esos datos, pero no cambia tu rama ni tus archivos. Gracias a eso Grove sabe cuántos commits tienes por subir (↑) o por traer (↓).", when: "Siempre que quieras saber si alguien subió algo. No tiene riesgo.", how: "Botón superior «Fetch origin», o ⌘/Ctrl+⇧+F. Grove también lo hace solo en segundo plano al abrir la app y al volver a ella." },
  { id: "pull", icon: "arrowDown", group: "Sincronizar", title: "Pull", what: "Hace fetch y además trae esos commits a tu rama actual, fusionándolos con los tuyos.", when: "Cuando el botón superior dice «Pull origin» (hay commits nuevos en el remoto).", how: "Botón superior «Pull origin». Si hay conflictos se abre el diálogo para resolverlos.", note: "Si tienes cambios sin commitear en los mismos archivos, git puede negarse: haz commit o stash antes." },
  { id: "push", icon: "arrowUp", group: "Sincronizar", title: "Push", what: "Sube tus commits locales a GitHub para que los vean los demás.", when: "Después de hacer commit, cuando el botón dice «Push origin».", how: "Botón superior «Push origin». Antes de subir, Grove comprueba que el remoto no tenga novedades; si las tiene, te ofrece hacer pull.", note: "«Forzar push» (--force-with-lease) solo sobrescribe el remoto si nadie subió nada desde tu último fetch." },
  { id: "ff", icon: "arrowDown", group: "Sincronizar", title: "Avance rápido", what: "Si tu rama solo va por detrás del remoto (no tienes commits nuevos propios), traer los cambios es mover el puntero hacia delante: no hay fusión ni conflictos posibles.", when: "Es lo que usa «Actualizar grupo» para poner al día muchos repos a la vez sin riesgo.", how: "Botón de sincronizar (↻) en el encabezado de cada grupo de la lista de repositorios.", note: "Los repos con commits propios sin subir o con la rama divergida no se tocan: se avisa para revisarlos a mano." },
  { id: "merge", icon: "gitMerge", group: "Ramas y commits", title: "Merge (fusionar)", what: "Une en tu rama actual todos los commits de otra rama que aún no tiene. Se crea un commit de fusión que junta ambos historiales.", when: "Cuando una rama terminó (una función, un arreglo) y quieres incorporarla a la principal.", how: "Abre el menú de ramas, pulsa el icono de fusionar junto a la rama que quieres traer y confirma. Estarás recibiendo en la rama actual.", note: "Si ambas ramas cambiaron las mismas líneas, se abre el diálogo de conflictos. Puedes abortar y todo vuelve a como estaba." },
  { id: "cherry", icon: "cherry", group: "Ramas y commits", title: "Cherry-pick", what: "Copia UN commit concreto de otra rama a tu rama actual, como un commit nuevo. El original se queda donde estaba.", when: "Cuando necesitas solo un arreglo de otra rama sin traerte todo lo demás que tiene.", how: "1) Colócate en la rama que va a RECIBIR el commit.  2) En Historial, elige arriba la rama que tiene el commit.  3) Clic derecho en el commit → Cherry-pick.", note: "Por defecto solo ves lo que tu rama aún no tiene. Si el commit ya está en tu rama actual, el cherry-pick no tiene sentido y no se ofrece." },
  { id: "revert", icon: "undo", group: "Ramas y commits", title: "Revert (revertir)", what: "Crea un commit nuevo que deshace los cambios de otro commit. No borra historial.", when: "Para deshacer algo que ya se subió y otros pueden tener. Es la forma segura de «echar atrás».", how: "En Historial (de la rama actual), clic derecho en el commit → Revertir este commit." },
  { id: "undo", icon: "undo", group: "Ramas y commits", title: "Deshacer último commit", what: "Quita tu último commit local y devuelve sus cambios al área de cambios, con su mensaje listo para editar.", when: "Te equivocaste de mensaje o de archivos y todavía no lo subiste.", how: "Aparece bajo el commit recién hecho (Deshacer / Editar).", note: "Solo para commits que aún no subiste." },
  { id: "rebase", icon: "arrowDown", group: "Sincronizar", title: "Pull con rebase", what: "En vez de crear un commit de fusión al traer cambios, reaplica tus commits locales encima de los del remoto. El historial queda en línea recta, más fácil de leer.", when: "Si tu equipo prefiere un historial lineal, o te molestan los commits «Merge branch…» al hacer pull.", how: "Menú desplegable junto al botón de sincronizar → «Pull con rebase». Si hay conflictos, el diálogo te los muestra uno a uno; con varios commits puede pedirte resolver más de una vez.", note: "Si ya habías subido esos commits y alguien más los usa, no reescribas su historia: en ese caso mejor el pull normal. Tus cambios sin commitear se guardan y se devuelven solos." },
  { id: "compare", icon: "gitMerge", group: "Ramas y commits", title: "Comparar ramas", what: "Muestra qué cambia una rama respecto a otra: cuántos commits tiene de más o de menos y el diff de cada archivo, calculado desde su ancestro común (igual que un Pull Request).", when: "Antes de fusionar o de abrir un Pull Request, para ver qué le falta o qué le sobra a una rama.", how: "Menú de ramas → «Comparar ramas…», o ⌘/Ctrl+K → «Comparar ramas…». Elige base y rama a comparar; pulsa un archivo para ver su diff." },
  { id: "hunk", icon: "diffModified", group: "Ramas y commits", title: "Elegir bloques y líneas", what: "En un archivo modificado puedes decidir qué entra al commit: línea a línea con la casilla de cada línea, o un bloque entero con la casilla de su cabecera «@@».", when: "Cuando un archivo mezcla cambios de dos cosas distintas y quieres commits separados.", how: "Pestaña Cambios → abre el archivo → marca o desmarca líneas o bloques. La casilla del bloque queda a medias si solo hay algunas líneas marcadas." },
  { id: "palette", icon: "search", group: "Atajos", title: "Paleta de comandos", what: "Un buscador único para saltar a cualquier repositorio o rama y lanzar acciones (fetch, nueva rama, abrir en el editor…) escribiendo solo unas letras.", when: "Cuando tienes muchos repositorios o ramas y moverte por los menús es lento.", how: "Pulsa ⌘/Ctrl+K, escribe, muévete con ↑↓ y confirma con ⏎. Sin escribir nada, muestra primero las acciones y los repos con novedades." },
  { id: "reflog", icon: "history", group: "Ramas y commits", title: "Recuperar (reflog)", what: "Git apunta todos los sitios donde ha estado HEAD. Aunque un reset, un borrado de rama o una operación abortada te haga «perder» commits, siguen ahí durante semanas.", when: "Hiciste un reset y te arrepentiste, borraste una rama por error o algo salió mal.", how: "Historial → «Recuperar…». Elige el punto al que quieres volver y pulsa «Crear rama…»: recuperas todo ese trabajo en una rama nueva sin tocar nada más." },
  { id: "reset", icon: "undo", group: "Ramas y commits", title: "Volver a un commit (reset)", what: "Mueve la rama actual a un commit anterior. Puedes conservar los cambios de los commits quitados (vuelven a «Cambios») o descartarlos por completo.", when: "Quieres rehacer los últimos commits, o abandonar trabajo que no sirvió.", how: "Historial → clic derecho en el commit → «Volver a este commit…».", note: "«Descartar todo» también borra tus cambios sin commitear. Los commits quitados se pueden recuperar con «Recuperar». Si ya los subiste, tendrás que forzar el push." },
  { id: "branchat", icon: "gitBranch", group: "Ramas y commits", title: "Crear rama desde un commit", what: "Crea una rama que empieza justo en un commit que elijas del historial, sin tener que estar en él.", when: "Quieres retomar trabajo desde un punto anterior o guardar un commit antes de tocar nada.", how: "Historial → clic derecho en el commit → «Crear rama aquí…». Puedes cambiar a ella al crearla." },
  { id: "stale", icon: "gitBranch", group: "Ramas y commits", title: "Ramas obsoletas", what: "Ramas locales cuya versión en GitHub ya se borró (normalmente tras fusionar un Pull Request). Se quedan en tu equipo hasta que las limpies.", when: "Cuando el menú de ramas muestra «remota eliminada» o «sin fusionar».", how: "Menú de ramas → «Limpiar…». Las fusionadas vienen marcadas porque borrarlas no pierde nada; las «sin fusionar» tienen commits que no están en ninguna otra rama, así que no se marcan solas.", note: "Grove limpia solo los registros de worktrees cuya carpeta se borró. Si el worktree estaba en un disco o recurso de red desconectado, lo conserva y te lo avisa: solo lo olvida si tú lo pides." },
  { id: "tag", icon: "tag", group: "Ramas y commits", title: "Tag (etiqueta)", what: "Una marca fija sobre un commit, normalmente para señalar versiones (v1.0.0). A diferencia de una rama, no se mueve.", when: "Al publicar una versión o querer volver a un punto concreto fácilmente.", how: "Historial → clic derecho en un commit → Crear tag aquí. Con mensaje es «anotado»; la casilla lo sube a origin." },
  { id: "stash", icon: "archive", group: "Ramas y commits", title: "Stash", what: "Guarda tus cambios sin commitear en un cajón y deja la carpeta limpia, para poder recuperarlos después.", when: "Necesitas cambiar de rama o hacer pull y no quieres hacer commit de algo a medias.", how: "Botón «Stash» de la barra superior. Desde ahí también puedes aplicarlos o borrarlos." },
  { id: "worktree", icon: "fileDirectory", group: "Ramas y commits", title: "Worktree", what: "Una segunda carpeta del mismo repositorio con otra rama abierta, para trabajar en dos ramas a la vez sin ir cambiando de una a otra.", when: "Tienes que atender un arreglo urgente sin tocar lo que llevas a medias.", how: "Botón de worktrees de la barra superior." },
  { id: "conflict", icon: "warn", group: "Conflictos", title: "Conflictos", what: "Ocurren cuando dos versiones cambian las mismas líneas de un archivo y git no puede decidir solo cuál conservar.", when: "Pueden aparecer en un pull, merge, cherry-pick o revert.", how: "Se abre un diálogo: para cada archivo elige «Mía» (la versión de tu rama), «Suya» (la que llega) o «Ya resuelto» (si lo editaste a mano). Luego pulsa Concluir.", note: "Abortar cancela la operación y deja todo exactamente como estaba. Siempre es seguro." },
  { id: "news", icon: "filter", group: "Repositorios", title: "Novedades", what: "Filtra la lista para ver solo los repos con algo pendiente: cambios sin commitear, commits por subir (↑) o por traer (↓).", when: "Para saber de un vistazo en qué repos tienes trabajo pendiente.", how: "Botón «Novedades» junto al filtro de la lista de repositorios." },
  { id: "group", icon: "repo", group: "Repositorios", title: "Actualizar grupo", what: "Hace fetch de todos los repos de un grupo en paralelo y adelanta los que se pueden adelantar sin riesgo.", when: "Al empezar el día, para poner al día todos los proyectos de un cliente.", how: "Botón ↻ en el encabezado del grupo. Al terminar verás un resumen." },
  { id: "keys", icon: "code", group: "Atajos", title: "Atajos de teclado", what: "⌘/Ctrl+⏎  Hacer commit\n⌘/Ctrl+⇧+P  Sincronizar (fetch, pull o push según el estado)\n⌘/Ctrl+⇧+F  Fetch\n⌘/Ctrl+1 / 2 / 3  Cambios / Historial / Pull Requests\nEsc  Cerrar el diálogo o menú abierto", when: "", how: "" },
];
// Puntuación de coincidencia para la paleta: subcadena (mejor cuanto antes empiece) o letras en orden; -1 = no coincide.
function fuzzy(q: string, t: string): number {
  q = q.toLowerCase(); t = t.toLowerCase();
  const i = t.indexOf(q);
  if (i >= 0) return 1000 - i - (t.length - q.length) * 0.01;
  let at = 0, last = -1, gaps = 0;
  for (const ch of q) {
    const j = t.indexOf(ch, at);
    if (j < 0) return -1;
    if (last >= 0) gaps += j - last - 1;
    last = j; at = j + 1;
  }
  return 500 - gaps;
}
type PalItem = { id: string; group: string; icon: IconName; label: string; hint?: string; run: () => void };
const OP_WHY: Record<string, string> = {
  merge: "Al unir las dos ramas, algunos archivos fueron cambiados en ambas y git no sabe cuál versión conservar.",
  "cherry-pick": "Estás copiando un commit a esta rama, pero algunos de sus archivos cambiaron aquí de otra forma y git no sabe cuál versión conservar.",
  revert: "Para deshacer ese commit hay que modificar archivos que cambiaron después, y git no sabe cuál versión conservar.",
  rebase: "Se están reaplicando tus commits encima de los del remoto y uno de ellos toca las mismas líneas. «Mía» es tu commit; «Suya», lo que ya había en el remoto. Si tienes varios commits, puede pedirte resolver más de una vez.",
};
const AUTH_ERR = /could not read (Username|Password)|terminal prompts disabled|Authentication failed|Invalid username or password|HTTP Basic: Access denied/i;
const [ghUserNow, setGhUserNow] = createSignal(false);
// Tus propios commits (correo = el de tu git config) muestran tu foto de la sesión, aunque ese correo no esté vinculado en GitHub.
// Se compara también por nombre (mismo autor con otro correo, p. ej. uno viejo); lo que GitHub resuelva por API manda sobre esto.
const [selfAvatar, setSelfAvatar] = createSignal<{ email: string; name: string; url: string }>();
const isSelf = (name: string, email?: string) => {
  const me = selfAvatar();
  if (!me) return false;
  return (!!email && email.toLowerCase() === me.email) || (!!me.name && name.trim().toLowerCase() === me.name);
};
// correo -> foto devuelta por la API de GitHub (solo con sesión iniciada)
const [apiAvatars, setApiAvatars] = createSignal<Record<string, string>>({});
const ghAvatar = (email?: string): string | undefined => {
  const m = /^(?:(\d+)\+)?([A-Za-z0-9-]+)@users\.noreply\.github\.com$/i.exec(email ?? "");
  if (!m) return undefined;
  return m[1] ? `https://avatars.githubusercontent.com/u/${m[1]}?s=96&v=4` : `https://github.com/${m[2]}.png?size=96`;
};

const hue = (s: string) => [...s].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 7);
function Avatar(props: { name: string; email?: string; size?: number; src?: string }) {
  const [broken, setBroken] = createSignal(false);
  const n = () => props.name.trim();
  const ini = () => (n().split(/\s+/).map((w) => w[0]).slice(0, 2).join("") || "?").toUpperCase();
  const sz = () => props.size ?? 24;
  const url = () => (broken() ? undefined : (props.src ?? apiAvatars()[props.email ?? ""] ?? (isSelf(props.name, props.email) ? selfAvatar()!.url : undefined) ?? (ghUserNow() ? ghAvatar(props.email) : undefined)));
  return (
    <span class="avatar" title={n()} style={{ width: `${sz()}px`, height: `${sz()}px`, "font-size": `${Math.round(sz() * 0.42)}px`, background: `hsl(${hue(n())} 45% 38%)` }}>
      <Show when={url()} fallback={ini()}>
        <img src={url()} alt="" referrerpolicy="no-referrer" onError={() => setBroken(true)} />
      </Show>
    </span>
  );
}

const ago = (s: number | null | undefined) => {
  if (!s) return "sin fetch aún";
  const m = Math.floor((Date.now() / 1000 - s) / 60);
  if (m < 1) return "hace un momento";
  if (m < 60) return `hace ${m} min`;
  if (m < 1440) return `hace ${Math.floor(m / 60)} h`;
  const d = Math.floor(m / 1440);
  if (d < 30) return `hace ${d} ${d === 1 ? "día" : "días"}`;
  const mo = Math.floor(d / 30);
  return mo < 12 ? `hace ${mo} ${mo === 1 ? "mes" : "meses"}` : `hace ${Math.floor(mo / 12)} a.`;
};

const when = (s: number) => ago(s);

const LINE_H = 18; // 12px * 1.5, fijo: requisito de la lista virtual
type DLine = { i: number; kind: "hunk" | "ctx" | "add" | "del" | "note"; old?: number; new?: number; text: string };
// Convierte el diff unificado en filas con numeración (como GitHub Desktop); oculta la cabecera
// de git. `i` conserva el índice original: es el que usan la selección por línea y el parche.
function diffLines(text: string): DLine[] {
  const out: DLine[] = [];
  let o = 0, n = 0, inHunk = false;
  text.split("\n").forEach((l, i) => {
    const h = /^@@ -(\d+)(?:,\d+)? \+(\d+)/.exec(l);
    if (h) { inHunk = true; o = +h[1]; n = +h[2]; out.push({ i, kind: "hunk", text: l }); }
    else if (!inHunk || (l === "" && i === text.split("\n").length - 1)) return;
    else if (l.startsWith("+")) out.push({ i, kind: "add", new: n++, text: l.slice(1) });
    else if (l.startsWith("-")) out.push({ i, kind: "del", old: o++, text: l.slice(1) });
    else if (l.startsWith("\\")) out.push({ i, kind: "note", text: l });
    else out.push({ i, kind: "ctx", old: o++, new: n++, text: l.slice(1) });
  });
  return out;
}

// Proveedores con API compatible con OpenAI; elegir uno rellena URL y modelo (editables).
const AI_PROVIDERS = [
  { name: "Groq (gratis)", base: "https://api.groq.com/openai/v1", model: "openai/gpt-oss-20b", keys: "https://console.groq.com/keys" },
  { name: "OpenAI", base: "https://api.openai.com/v1", model: "gpt-4o-mini", keys: "https://platform.openai.com/api-keys" },
  { name: "OpenRouter", base: "https://openrouter.ai/api/v1", model: "", keys: "https://openrouter.ai/keys" },
  { name: "Gemini", base: "https://generativelanguage.googleapis.com/v1beta/openai", model: "gemini-2.5-flash", keys: "https://aistudio.google.com/apikey" },
  { name: "Ollama", base: "http://localhost:11434/v1", model: "", keys: "" },
];

function DiffView(props: { text: string; checked?: (i: number) => boolean | undefined; onToggle?: (i: number) => void; hunk?: (i: number) => "all" | "some" | "none" | undefined; onHunk?: (i: number) => void }) {
  let box!: HTMLDivElement;
  const lines = createMemo(() => diffLines(props.text));
  const width = createMemo(() => lines().reduce((m, l) => Math.max(m, l.text.length), 0));
  const v = createVirtualizer({
    get count() { return lines().length; },
    getScrollElement: () => box,
    estimateSize: () => LINE_H,
    overscan: 30,
  });
  const gutter = () => (props.checked ? 28 : 0) + 88 + 20;
  return (
    <div class="diff" ref={box}>
      <div style={{ height: `${v.getTotalSize()}px`, width: `calc(${width()}ch + ${gutter() + 24}px)`, "min-width": "100%", position: "relative" }}>
        <For each={v.getVirtualItems()}>
          {(r) => {
            const l = () => lines()[r.index];
            return (
              <div class={`dl ${l().kind}`} style={{ position: "absolute", top: 0, left: 0, right: 0, height: `${LINE_H}px`, transform: `translateY(${r.start}px)` }}>
                <Show when={l().kind !== "hunk"} fallback={<>
                  <Show when={props.hunk?.(l().i)} keyed>
                    {(st) => <span class="gut hunkgut"><input type="checkbox" title="Marcar o desmarcar todo este bloque" checked={st === "all"} ref={(el) => (el.indeterminate = st === "some")} onChange={() => props.onHunk!(l().i)} /></span>}
                  </Show>
                  <span class="htext">{l().text}</span>
                </>}>
                  <Show when={props.checked}>
                    <span class="gut">
                      <Show when={props.checked!(l().i) !== undefined}>
                        <input type="checkbox" checked={props.checked!(l().i)} onChange={() => props.onToggle!(l().i)} />
                      </Show>
                    </span>
                  </Show>
                  <span class="num">{l().old ?? ""}</span><span class="num">{l().new ?? ""}</span>
                  <span class="sign">{l().kind === "add" ? "+" : l().kind === "del" ? "−" : ""}</span>
                  <span class="code">{l().text || " "}</span>
                </Show>
              </div>
            );
          }}
        </For>
      </div>
    </div>
  );
}

type Row = { kind: "hunk" | "ctx" | "chg"; l?: string; r?: string };
// Empareja cada bloque de `-` con el `+` que le sigue (como GitHub Desktop en lado a lado).
function splitRows(text: string): Row[] {
  const rows: Row[] = [];
  let dels: string[] = [], adds: string[] = [], inHunk = false;
  const flush = () => {
    for (let i = 0; i < Math.max(dels.length, adds.length); i++) rows.push({ kind: "chg", l: dels[i], r: adds[i] });
    dels = []; adds = [];
  };
  for (const l of text.split("\n")) {
    if (l.startsWith("@@")) { flush(); inHunk = true; rows.push({ kind: "hunk", l }); }
    else if (!inHunk) continue;
    else if (l.startsWith("-")) dels.push(l.slice(1));
    else if (l.startsWith("+")) adds.push(l.slice(1));
    else if (l.startsWith("\\")) continue;
    else { flush(); if (l !== "") rows.push({ kind: "ctx", l: l.slice(1), r: l.slice(1) }); }
  }
  flush();
  return rows;
}

function SplitView(props: { text: string }) {
  let box!: HTMLDivElement;
  const rows = createMemo(() => splitRows(props.text));
  const v = createVirtualizer({
    get count() { return rows().length; },
    getScrollElement: () => box,
    estimateSize: () => LINE_H,
    overscan: 30,
  });
  return (
    <div class="diff" ref={box}>
      <div style={{ height: `${v.getTotalSize()}px`, position: "relative" }}>
        <For each={v.getVirtualItems()}>
          {(it) => {
            const r = () => rows()[it.index];
            return (
              <div class="srow" style={{ transform: `translateY(${it.start}px)`, height: `${LINE_H}px` }}>
                <Show when={r().kind !== "hunk"} fallback={<div class="dl hunk wide">{r().l}</div>}>
                  <div class="dl half" classList={{ del: r().l !== undefined && r().kind === "chg", empty: r().l === undefined }}>{r().l ?? " "}</div>
                  <div class="dl half" classList={{ add: r().r !== undefined && r().kind === "chg", empty: r().r === undefined }}>{r().r ?? " "}</div>
                </Show>
              </div>
            );
          }}
        </For>
      </div>
    </div>
  );
}

const IMG = /\.(png|jpe?g|gif|webp|svg)$/i;
const isBinary = (t: string) => /^Binary files .* differ$/m.test(t);

function ImageView(props: { repo: string; file: string }) {
  const [v] = createResource(() => [props.repo, props.file] as const, ([r, f]) => commands.imageVersions(r, f));
  return (
    <div class="imgs">
      <Show when={v.error}><p class="err">{String(v.error)}</p></Show>
      <Show when={v()} keyed>
        {(x) => (
          <>
            <figure><figcaption>Antes</figcaption><Show when={x[0]} fallback={<p class="muted">No existía</p>}><img src={x[0]!} /></Show></figure>
            <figure><figcaption>Ahora</figcaption><Show when={x[1]} fallback={<p class="muted">Eliminada</p>}><img src={x[1]!} /></Show></figure>
          </>
        )}
      </Show>
    </div>
  );
}

// Índices de líneas +/- dentro de hunks (lo que se puede elegir línea a línea).
// Líneas elegibles agrupadas por bloque (clave: índice de su cabecera `@@`).
const hunkLines = (text: string): Map<number, number[]> => {
  const m = new Map<number, number[]>();
  let h = -1;
  text.split("\n").forEach((l, i) => {
    if (l.startsWith("@@")) { h = i; m.set(h, []); }
    else if (h >= 0 && (l.startsWith("+") || l.startsWith("-"))) m.get(h)!.push(i);
  });
  return m;
};
const selectable = (text: string): number[] => {
  const out: number[] = [];
  let inHunk = false;
  text.split("\n").forEach((l, i) => {
    if (l.startsWith("@@")) inHunk = true;
    else if (inHunk && (l.startsWith("+") || l.startsWith("-"))) out.push(i);
  });
  return out;
};

export default function App() {
  const [repos, { refetch: reload }] = createResource(() => commands.listRepos());
  const [current, setCurrent] = createSignal<string>();
  const [menu, setMenu] = createSignal<"repo" | "branch" | "wt" | null>(null);
  const [fetching, setFetching] = createSignal(false);
  const [off, setOff] = createSignal<Set<string>>(new Set()); // archivos desmarcados
  const [summary, setSummary] = createSignal("");
  const [aiCfg, { refetch: refAi }] = createResource(() => commands.aiSettings());
  const aiOn = () => !!aiCfg()?.base_url;
  // Diálogo «Configuración de IA»; `key` vacía al guardar = conservar la clave guardada.
  const [aiDlg, setAiDlg] = createSignal<{ base: string; model: string; key: string }>();
  const openAiDlg = () => setAiDlg({ base: aiCfg()?.base_url || AI_PROVIDERS[0].base, model: aiCfg()?.model || AI_PROVIDERS[0].model, key: "" });
  async function saveAi(off = false) {
    const d = aiDlg()!;
    setErr("");
    try { await commands.setAiSettings(off ? "" : d.base, off ? "" : d.model, off ? "" : d.key.trim() || null); setAiDlg(undefined); refAi(); }
    catch (e) { setErr(String(e)); }
  }
  const [aiBusy, setAiBusy] = createSignal(false);
  async function aiMessage() {
    setErr(""); setAiBusy(true);
    try { const m = await commands.aiCommitMessage(cur()!, included()); setSummary(m.summary); setBody(m.body); setJustCommitted(undefined); }
    catch (e) { setErr(String(e)); }
    setAiBusy(false);
  }
  const [body, setBody] = createSignal("");
  const [blocked, setBlocked] = createSignal<string>(); // rama destino si hay cambios que chocan
  const [committing, setCommitting] = createSignal(false);
  const [amend, setAmend] = createSignal(false);
  const [sideBySide, setSideBySide] = createSignal(false);
  // Selección parcial por archivo: índices de líneas del diff incluidas. Ausente = archivo completo.
  const [partial, setPartial] = createSignal<Map<string, Set<number>>>(new Map());
  const [confirmDiscard, setConfirmDiscard] = createSignal(false);
  const [discardFiles, setDiscardFiles] = createSignal<string[]>(); // desde el clic derecho: solo ese archivo; si no, los marcados
  const toDiscard = () => discardFiles() ?? included();
  const [fctx, setFctx] = createSignal<{ x: number; y: number; path: string; kind: string }>();
  const fctxDo = async (f: () => unknown) => { setFctx(undefined); try { await f(); } catch (x) { setErr(String(x)); } };
  const absPath = (rel: string) => `${cur()}${sepOf(cur() ?? "")}${rel}`;
  const [filter, setFilter] = createSignal("");
  const [tab, setTab] = createSignal<"changes" | "history" | "prs">("changes");
  const [file, setFile] = createSignal<string>();
  const [err, setErr] = createSignal("");
  const SIDE_KEY = "grove.side";
  const [side, setSide] = createSignal(
    (() => { try { return Math.min(700, Math.max(260, +(localStorage.getItem(SIDE_KEY) ?? 400))); } catch { return 400; } })(),
  );
  function startResize(e: MouseEvent) {
    e.preventDefault();
    const move = (m: MouseEvent) => setSide(Math.min(700, Math.max(260, m.clientX)));
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      try { localStorage.setItem(SIDE_KEY, String(side())); } catch { /* preferencia de esta sesión */ }
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  }
  const [creating, setCreating] = createSignal(false);
  const [newBranch, setNewBranch] = createSignal("");
  // Tarjeta de "acabas de hacer commit" (Deshacer / Enmendar), solo justo después de commitear.
  const [justCommitted, setJustCommitted] = createSignal<string>();

  // Primer repo de la lista (más reciente) si no hay uno elegido.
  const cur = () => current() ?? repos()?.[0]?.path;
  const [changes, { refetch }] = createResource(cur, (p) => commands.changes(p));
  // Historial paginado: 100 commits por página, la siguiente al acercarse al final.
  const PAGE = 100;
  const [commits, setCommits] = createSignal<Commit[]>([]);
  const [hasMore, setHasMore] = createSignal(true);
  const [fAuthor, setFAuthor] = createSignal("");
  const [fSince, setFSince] = createSignal("");
  const [fUntil, setFUntil] = createSignal("");
  const [fSearch, setFSearch] = createSignal("");
  const [fFile, setFFile] = createSignal("");
  const [fRev, setFRev] = createSignal(""); // historial de otra rama (vacío = la actual)
  const [fOnlyNew, setFOnlyNew] = createSignal(true); // con otra rama: solo lo que la actual aún no tiene
  const filtered = () => !!(fAuthor() || fSince() || fUntil() || fSearch() || fFile() || fRev());
  const clearFilters = () => { setFAuthor(""); setFSince(""); setFUntil(""); setFSearch(""); setFFile(""); setFRev(""); setFOnlyNew(true); };
  const [histBranches, { refetch: refHistBranches }] = createResource(() => (tab() === "history" ? cur() : undefined), (p) => commands.branches(p));
  let searchTimer: number | undefined; // el texto se aplica 300 ms después de dejar de teclear
  const onSearch = (v: string) => { clearTimeout(searchTimer); searchTimer = window.setTimeout(() => setFSearch(v.trim()), 300); };
  onCleanup(() => clearTimeout(searchTimer));
  const [authors] = createResource(() => (tab() === "history" ? cur() : undefined), (p) => commands.authors(p));
  let loadingHistory = false;
  const [histBusy, setHistBusy] = createSignal(false);
  async function loadHistory(reset: boolean) {
    const p = cur();
    if (!p || loadingHistory || (!reset && !hasMore())) return;
    loadingHistory = true; setHistBusy(true);
    try {
      const page = await commands.history(p, reset ? 0 : commits().length, PAGE, { author: fAuthor() || null, since: fSince() || null, until: fUntil() || null, search: fSearch() || null, file: fFile() || null, rev: fRev() || null, only_new: !!fRev() && fOnlyNew() });
      if (p !== cur()) return;
      setCommits(reset ? page : [...commits(), ...page]);
      setHasMore(page.length === PAGE);
    } catch (e) {
      // Repo sin commits: lista vacía, no es un error para el usuario.
      if (reset) { setCommits([]); setHasMore(false); }
      if (!String(e).includes("unborn") && !String(e).includes("HEAD")) setErr(String(e));
    } finally { loadingHistory = false; setHistBusy(false); }
  }
  const refHistory = () => loadHistory(true);
  createEffect(on(cur, () => clearFilters(), { defer: true }));
  // Al cambiar un filtro se recarga desde cero (loadHistory descarta si ya hay una carga en curso).
  createEffect(on([fAuthor, fSince, fUntil, fSearch, fFile, fRev, fOnlyNew], () => { setSel(undefined); refHistory(); }, { defer: true }));

  // ---- Cuenta de GitHub (OAuth device flow) ----
  const [ghUser, setGhUser] = createSignal<{ login: string; avatar_url: string } | null>(null);
  const [ghCode, setGhCode] = createSignal<{ user_code: string; verification_uri: string }>();
  let loginRun = 0; // invalida un sondeo en curso al cancelar
  createEffect(() => setGhUserNow(!!ghUser()));
  commands.ghUser().then(setGhUser).catch(() => {});
  async function ghLogin() {
    setErr("");
    const run = ++loginRun;
    try {
      const c = await commands.ghLoginStart();
      setGhCode(c);
      openUrl(c.verification_uri).catch(() => {});
      let wait = c.interval;
      while (run === loginRun) {
        await new Promise((r) => setTimeout(r, wait * 1000));
        if (run !== loginRun) return;
        const r = await commands.ghLoginPoll(c.device_code);
        if (r.status === "slow") wait += 5;
        if (r.status === "done") { setGhUser(r.user); break; }
      }
    } catch (e) { setErr(String(e)); }
    if (run === loginRun) setGhCode(undefined);
  }
  const ghCancel = () => { loginRun++; setGhCode(undefined); };
  async function ghLogout() { await commands.ghLogout(); setApiAvatars({}); setGhUser(null); }
  // Pide la foto de los autores aún sin resolver: hasta 3 commits por correo (los locales sin publicar no existen en GitHub).
  const asked = new Set<string>();
  const avatarErrShown = new Set<string>(); // un aviso por repo, no uno por página de historial
  createEffect(() => {
    const p = cur();
    if (!ghUser() || !p) return;
    const by = new Map<string, string[]>();
    for (const c of commits()) {
      if (!c.email || asked.has(`${p}|${c.email}`)) continue;
      by.set(c.email, [...(by.get(c.email) ?? []), c.id].slice(0, 3));
    }
    const picks = [...by.entries()];
    for (const [e] of picks) asked.add(`${p}|${e}`);
    if (!picks.length) return;
    commands.ghAvatars(p, picks)
      .then((r) => setApiAvatars((m) => ({ ...m, ...Object.fromEntries(r) })))
      .catch((e) => {
        for (const [em] of picks) asked.delete(`${p}|${em}`); // se reintenta al reabrir el repo
        if (!avatarErrShown.has(p)) { avatarErrShown.add(p); setErr(String(e)); }
      });
  });
  createEffect(() => { cur(); setSel(undefined); setSelFile(undefined); setCommits([]); setHasMore(true); loadHistory(true); });
  const [sel, setSel] = createSignal<string>(); // commit elegido en Historial
  const [selFile, setSelFile] = createSignal<string>();
  const [bodyOpen, setBodyOpen] = createSignal(false);
  createEffect(() => { sel(); setBodyOpen(false); });
  const [detail] = createResource(() => (cur() && sel() ? ([cur()!, sel()!] as const) : undefined), ([p, id]) => commands.commitInfo(p, id));
  const [cdiff] = createResource(
    () => (cur() && sel() && selFile() ? ([cur()!, sel()!, selFile()!] as const) : undefined),
    ([p, id, f]) => commands.commitDiff(p, id, f),
  );
  createEffect(() => { const d = detail(); if (d) setSelFile(d.files[0]?.path); });
  const [ident] = createResource(cur, (p) => commands.identity(p));
  createEffect(() => {
    const id = ident(), u = ghUser();
    setSelfAvatar(id && u ? { email: (id[1] ?? "").toLowerCase(), name: (id[0] ?? "").trim().toLowerCase(), url: u.avatar_url } : undefined);
  });
  const [vis, setVis] = createSignal(""); // filtro de la lista de cambios
  const [diff, { refetch: refDiff }] = createResource(
    () => (cur() && file() ? ([cur()!, file()!] as const) : undefined),
    ([p, f]) => commands.fileDiff(p, f),
  );
  const included = createMemo(() => (changes() ?? []).filter((c) => !off().has(c.path)).map((c) => c.path));
  // Como GitHub Desktop: con un solo archivo marcado, resumen por defecto sin gastar IA.
  const autoSummary = createMemo(() => {
    const on = (changes() ?? []).filter((c) => !off().has(c.path));
    if (on.length !== 1 || amend()) return "";
    const name = on[0].path.split(/[\\/]/).pop();
    return `${on[0].kind === "Added" ? "Crea" : on[0].kind === "Removed" ? "Elimina" : "Actualiza"} ${name}`;
  });
  const finalSummary = () => summary().trim() || autoSummary();
  const visible = createMemo(() => (changes() ?? []).filter((c) => c.path.toLowerCase().includes(vis().toLowerCase())));
  // Las listas viven dentro de <Show> y se montan al cambiar de pestaña: el virtualizador debe
  // enterarse del nuevo elemento (si no, queda vacío hasta que cambie el conteo).
  // Historial sin virtualizar: se pagina de 100 en 100 y 50 px/fila es barato de pintar.
  const onHistScroll = (e: Event & { currentTarget: HTMLElement }) => {
    const el = e.currentTarget;
    if (el.scrollHeight - el.scrollTop - el.clientHeight < 600) loadHistory(false);
  };
  // Como GitHub Desktop: al abrir un repo con cambios se muestra el diff del primer archivo (una vez por repo).
  let autoFor = "";
  createEffect(() => {
    const p = cur(), l = changes();
    if (!p || changes.loading || !l || autoFor === p) return;
    autoFor = p;
    if (l.length && !file()) setFile(l[0].path);
  });
  const allOn = () => included().length === (changes()?.length ?? 0);
  const dropPartial = (p?: string) => {
    const m = new Map(partial());
    p === undefined ? m.clear() : m.delete(p);
    setPartial(m);
  };
  const toggle = (p: string) => {
    const n = new Set(off());
    n.has(p) ? n.delete(p) : n.add(p);
    setOff(n);
    dropPartial(p);
  };
  const toggleAll = () => {
    setOff(allOn() ? new Set((changes() ?? []).map((c) => c.path)) : new Set<string>());
    dropPartial();
  };

  // Líneas elegibles solo en archivos modificados (los nuevos/borrados van enteros).
  const canPick = () => changes()?.find((c) => c.path === file())?.kind === "Modified";
  const pickable = createMemo(() => (diff() ? selectable(diff()!) : []));
  const pickableSet = createMemo(() => new Set(pickable()));
  const lineChecked = (i: number): boolean | undefined => {
    const f = file();
    if (!f || !canPick() || !pickableSet().has(i)) return undefined;
    if (off().has(f)) return false;
    return partial().get(f)?.has(i) ?? true;
  };
  // Selección actual de líneas del archivo abierto (todas si no hay selección parcial).
  const selNow = () => { const f = file()!; return off().has(f) ? new Set<number>() : new Set(partial().get(f) ?? pickable()); };
  const hunks = createMemo(() => (diff() && canPick() ? hunkLines(diff()!) : new Map<number, number[]>()));
  const hunkState = (i: number): "all" | "some" | "none" | undefined => {
    const ls = hunks().get(i);
    if (!ls?.length) return undefined;
    const cur = selNow(), n = ls.filter((x) => cur.has(x)).length;
    return n === 0 ? "none" : n === ls.length ? "all" : "some";
  };
  function toggleHunk(i: number) {
    const ls = hunks().get(i) ?? [], cur = selNow(), on = ls.every((x) => cur.has(x));
    ls.forEach((x) => (on ? cur.delete(x) : cur.add(x)));
    applySel(cur);
  }
  function toggleLine(i: number) {
    const cur = selNow();
    cur.has(i) ? cur.delete(i) : cur.add(i);
    applySel(cur);
  }
  function applySel(cur: Set<number>) {
    const f = file()!;
    const all = pickable();
    const o = new Set(off());
    const m = new Map(partial());
    if (cur.size === 0) { o.add(f); m.delete(f); }
    else if (cur.size === all.length) { o.delete(f); m.delete(f); }
    else { o.delete(f); m.set(f, cur); }
    setOff(o); setPartial(m);
  }
  // Si el archivo cambió en disco, los índices elegidos ya no valen: volver a "completo".
  const seen = new Map<string, string>();
  createEffect(() => {
    const f = file(), d = diff();
    if (!f || d === undefined) return;
    if (seen.has(f) && seen.get(f) !== d && partial().has(f)) dropPartial(f);
    seen.set(f, d);
  });

  // Watcher del backend (notify + debounce, ignora lo de .gitignore): refresca sin polling.
  const [watching, setWatching] = createSignal(true);
  createEffect(() => {
    const p = cur();
    if (!p) return;
    setWatching(true);
    commands.watchRepo(p).catch((e) => { setWatching(false); setErr(String(e)); });
  });
  // Plan B si el SO no deja vigilar (p. ej. límite de inotify): refrescar al volver a la ventana.
  const onFocus = () => { if (!watching() && cur()) { refetch(); refSync(); } setAutoTick((n) => n + 1); }; // el fetch automático se limita solo por antigüedad
  window.addEventListener("focus", onFocus);
  // Escape cierra lo más "alto": diálogos, desplegables; si no hay nada, deselecciona el archivo (vuelve el panel de stash).
  const onKey = (e: KeyboardEvent) => {
    if (e.key !== "Escape") return;
    if (prNew() && !prBusy()) setPrNew(false);
    else if (syncMenu()) setSyncMenu(false);
    else if (ctx()) setCtx(undefined);
    else if (infoPath()) setInfoPath(undefined);
    else if (editRepo()) setEditRepo(undefined);
    else if (aiDlg()) setAiDlg(undefined);
    else if (wtDlg()) setWtDlg(false);
    else if (creating()) setCreating(false);
    else if (cloning() && !cloneBusy()) setCloning(null);
    else if (rejected()) setRejected(false);
    else if (blocked()) setBlocked(undefined);
    else if (fctx()) setFctx(undefined);
    else if (confirmDel()) setConfirmDel(undefined);
    else if (palOpen()) setPalOpen(false);
    else if (cmpOpen()) setCmpOpen(false);
    else if (branchAt() && !opBusy()) setBranchAt(undefined);
    else if (resetDlg() && !opBusy()) setResetDlg(undefined);
    else if (reflogOpen()) setReflogOpen(false);
    else if (helpOpen()) setHelpOpen(false);
    else if (opAsk()) setOpAsk(undefined);
    else if (sync()?.merging && resolveOpen()) setResolveOpen(false);
    else if (cctx()) setCctx(undefined);
    else if (tagDlg() && !tagBusy()) setTagDlg(undefined);
    else if (cleanDlg() && !cleanBusy()) setCleanDlg(undefined);
    else if (ghostDlg()) setGhostDlg(false);
    else if (mergeBr()) setMergeBr(undefined);
    else if (confirmDiscard()) { setConfirmDiscard(false); setDiscardFiles(undefined); }
    else if (ghCode()) ghCancel();
    else if (menu()) { setMenu(null); setRenaming(undefined); }
    else if (tab() === "changes" && stashMode()) { if (stashEdit()) setStashEdit(undefined); else if (stashSel() !== undefined) setStashSel(undefined); else setStashMode(false); }
    else if (tab() === "changes" && file()) setFile(undefined);
    else return;
    e.preventDefault();
  };
  window.addEventListener("keydown", onKey);
  onCleanup(() => window.removeEventListener("keydown", onKey));
  // Atajos con Cmd (macOS) o Ctrl: ⏎ commit · ⇧P sincronizar (fetch/pull/push según el estado) · ⇧F fetch · 1/2/3 pestañas.
  const onShortcut = (e: KeyboardEvent) => {
    if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
    const k = e.key.toLowerCase();
    if (k === "k" && !e.shiftKey) { palOpen() ? setPalOpen(false) : openPal(); e.preventDefault(); return; }
    if (!cur()) return;
    if (k === "enter" && tab() === "changes" && finalSummary() && (included().length || amend()) && !committing()) doCommit();
    else if (e.shiftKey && k === "p" && !fetching()) doSync();
    else if (e.shiftKey && k === "f" && !fetching()) doSync(false, "fetch");
    else if (!e.shiftKey && ["1", "2", "3"].includes(k)) setTab((["changes", "history", "prs"] as const)[+k - 1]);
    else return;
    e.preventDefault();
  };
  window.addEventListener("keydown", onShortcut);
  onCleanup(() => window.removeEventListener("keydown", onShortcut));
  onCleanup(() => window.removeEventListener("focus", onFocus));
  const un = listen<{ path: string; meta: boolean }>("repo-changed", (e) => {
    if (e.payload.path !== cur() || Date.now() < quietUntil) return;
    refetch();
    if (file()) refDiff();
    if (e.payload.meta) { refBranch(); refHistory(); refSync(); }
  });
  onCleanup(() => un.then((f) => f()));
  const [prog, setProg] = createSignal<{ label: string; pct: number }>();
  const unp = listen<{ label: string; pct: number }>("net-progress", (e) => setProg(e.payload));
  onCleanup(() => unp.then((f) => f()));
  const autoOff = new Set<string>(); // archivos que se desmarcaron solos: si los marcas tú, no se vuelven a desmarcar
  createEffect(() => { cur(); setJustCommitted(undefined); setOff(new Set<string>()); autoOff.clear(); setPartial(new Map()); seen.clear(); setSummary(""); setBody(""); setAmend(false); });
  // `.DS_Store` llega desmarcado: si se escapó uno, no entra al commit salvo que lo marques a propósito.
  createEffect(() => {
    const l = changes();
    if (!l || changes.loading) return;
    const fresh = l.map((c) => c.path).filter((p) => /(^|[\\/])\.DS_Store$/.test(p) && !autoOff.has(p));
    if (!fresh.length) return;
    fresh.forEach((p) => autoOff.add(p));
    setOff((o) => new Set([...o, ...fresh]));
  });

  async function doCommit() {
    setCommitting(true); setErr("");
    try {
      await commands.commit(cur()!, included().map((f) => ({ file: f, lines: partial().has(f) ? [...partial().get(f)!] : null })), finalSummary(), body(), amend());
      setJustCommitted(finalSummary()); // antes de limpiar el formulario
      setSummary(""); setBody(""); setFile(undefined); setAmend(false); setPartial(new Map());
    } catch (e) { setErr(String(e)); }
    setCommitting(false);
    refetch(); refHistory(); refSync(); // el botón superior pasa a «Push origin» al haber commits por subir
  }
  async function toggleAmend(on: boolean) {
    setAmend(on);
    if (!on) return;
    setJustCommitted(undefined);
    try { const m = await commands.lastMessage(cur()!); setSummary(m.summary); setBody(m.body); } catch (e) { setErr(String(e)); }
  }
  async function undo() {
    setErr("");
    try { const m = await commands.undoCommit(cur()!); setSummary(m.summary); setBody(m.body); setJustCommitted(undefined); } catch (e) { setErr(String(e)); }
    refetch(); refHistory(); refSync();
  }
  async function doDiscard() {
    setConfirmDiscard(false); setErr("");
    try { await commands.discard(cur()!, toDiscard()); setFile(undefined); } catch (e) { setErr(String(e)); }
    setDiscardFiles(undefined);
    refetch();
  }
  const [branch, { refetch: refBranch }] = createResource(cur, (p) => commands.branch(p));
  const [branches, { refetch: refBranches }] = createResource(() => (menu() === "branch" ? cur() : undefined), (p) => commands.branches(p));
  // Ramas locales cuya remota ya no existe. Al pedirlas, el backend limpia además los worktrees rotos.
  const [stale, { refetch: refStale }] = createResource(() => (menu() === "branch" ? cur() : undefined), (p) => commands.staleBranches(p));
  // Worktrees ausentes por una unidad desconectada: se conservan y solo se olvidan si lo pides.
  const [ghosts, { refetch: refGhosts }] = createResource(() => (menu() === "branch" ? cur() : undefined), (p) => commands.ghostWorktrees(p));
  const [ghostDlg, setGhostDlg] = createSignal(false);
  async function forgetGhosts() {
    setErr("");
    try { await commands.forgetGhostWorktrees(cur()!); setGhostDlg(false); setNote("Worktrees ausentes olvidados."); } catch (e) { setErr(String(e)); }
    refGhosts(); refStale(); refBranches();
  }
  const staleOf = (name: string) => stale()?.find((s) => s.name === name);
  const [cleanDlg, setCleanDlg] = createSignal<Set<string>>();
  const [cleanBusy, setCleanBusy] = createSignal(false);
  async function cleanBranches() {
    const sel = cleanDlg();
    if (!sel?.size || cleanBusy()) return;
    setCleanBusy(true); setErr("");
    try { const n = await commands.cleanBranches(cur()!, [...sel]); setCleanDlg(undefined); setNote(`${n} ${n === 1 ? "rama borrada" : "ramas borradas"}.`); }
    catch (e) { setErr(String(e)); }
    setCleanBusy(false); refBranches(); refStale();
  }
  const branchGroups = createMemo(() => {
    const q = filter().toLowerCase();
    const all = (branches() ?? []).filter((b) => b.name.toLowerCase().includes(q));
    const byTime = [...all].sort((a, b) => b.time - a.time);
    const def = all.find((b) => !b.remote && (b.name === "main" || b.name === "master"));
    const rest = byTime.filter((b) => b !== def);
    const recent = rest.filter((b) => !b.remote).slice(0, 4);
    return { def, recent, other: rest.filter((b) => !recent.includes(b)) };
  });
  const [publishNew, setPublishNew] = createSignal(true);
  // ---- Worktrees ----
  const [wtDlg, setWtDlg] = createSignal(false);
  const [wtNew, setWtNew] = createSignal(true);
  const [wtBranch, setWtBranch] = createSignal("");
  const [wtParent, setWtParent] = createSignal("");
  const [wtBranches] = createResource(() => (wtDlg() && !wtNew() ? cur() : undefined), (p) => commands.branches(p));
  const sepOf = (p: string) => (p.includes("\\") && !p.includes("/") ? "\\" : "/");
  const dirname = (p: string) => p.replace(/[\\/]+$/, "").split(/[\\/]/).slice(0, -1).join(sepOf(p)) || sepOf(p);
  const wtDest = () => (wtParent() && wtBranch().trim() ? `${wtParent()}${sepOf(wtParent())}${name(cur() ?? "")}-${wtBranch().trim().replace(/[\\/]/g, "-").replace(/^origin-/, "")}` : "");
  function openWt() { setMenu(null); setWtNew(true); setWtBranch(""); setWtParent(dirname(cur() ?? "")); setWtDlg(true); }
  async function doWorktree() {
    setErr("");
    try { await commands.worktreeAdd(cur()!, wtDest(), wtBranch().trim(), wtNew()); setWtDlg(false); await pick(wtDest()); }
    catch (e) { setErr(String(e)); }
  }
  async function createBranch() {
    const n = newBranch().trim();
    if (!n) return;
    setErr("");
    try {
      await commands.createBranch(cur()!, n);
      if (publishNew()) await commands.push(cur()!, false).catch((e) => setErr(`Rama creada, pero no se pudo publicar: ${e}`));
      setMenu(null); setCreating(false); setNewBranch(""); refBranch(); refetch(); refHistory(); }
    catch (e) { setErr(String(e)); }
  }
  const [trees, { refetch: refTrees }] = createResource(() => (menu() === "wt" ? cur() : undefined), (p) => commands.worktrees(p));
  // Confirmación común a todo lo que borra (rama, worktree, stash, tag): `run` solo corre si se acepta.
  type Confirm = { title: string; body: string; label: string; run: () => unknown };
  const [confirmDel, setConfirmDel] = createSignal<Confirm>();
  async function wtRemove(path: string, force: boolean) {
    setErr("");
    try { await commands.wtRemove(cur()!, path, force); refTrees(); }
    catch (e) {
      if (force) { setErr(String(e)); return; }
      // Sin forzar git se niega si hay cambios sin commitear: se vuelve a preguntar, avisando de lo que se pierde.
      setConfirmDel({ title: "El worktree tiene cambios sin commitear", body: `Quitarlo borra su carpeta con esos cambios y no se pueden recuperar.\n\n${String(e)}`, label: "Quitar y perder cambios", run: () => wtRemove(path, true) });
    }
  }
  const askWtRemove = (w: { path: string; branch?: string | null }) =>
    setConfirmDel({ title: `¿Quitar el worktree «${w.branch ?? name(w.path)}»?`, body: `Se borra la carpeta ${w.path} de tu disco. La rama y sus commits se conservan.`, label: "Quitar worktree", run: () => wtRemove(w.path, false) });
  // ---- Borrar rama remota ----
  const [delRemote, setDelRemote] = createSignal<string>();
  async function doDelRemote() {
    const n = delRemote()!;
    setErr("");
    try { await commands.deleteRemoteBranch(cur()!, n); setDelRemote(undefined); refBranches(); } catch (e) { setErr(String(e)); setDelRemote(undefined); }
  }
  const [fetched, { mutate: setFetched }] = createResource(cur, (p) => commands.lastFetched(p));

  async function launch(t: "Editor" | "Terminal" | "Folder") {
    setErr("");
    try { await commands.openIn(t, cur()!); } catch (e) { setErr(String(e)); }
  }
  const [sync, { refetch: refSync }] = createResource(cur, (p) => commands.syncState(p).catch(() => undefined));
  // Último commit aún sin subir (como GitHub Desktop): sigue visible al reabrir la app para poder deshacerlo o editarlo.
  const [lastMsg] = createResource(() => (cur() && (sync()?.ahead ?? 0) > 0 ? ([cur()!, sync()] as const) : undefined), ([p]) => commands.lastMessage(p));
  const unpushed = () => justCommitted() ?? ((sync()?.ahead ?? 0) > 0 ? lastMsg()?.summary : undefined);
  // Un solo botón, como GitHub Desktop: publica, hace pull, push o fetch según el estado de la rama.
  const syncAct = () => {
    const s = sync();
    if (s && !s.has_origin) return { kind: "publish" as const, title: "Publicar repositorio", sub: "Súbelo a GitHub", icon: "arrowUp" as const };
    if (s?.has_origin && !s.upstream) return { kind: "push" as const, title: "Publicar rama", sub: "Sube la rama a origin", icon: "arrowUp" as const };
    if (s?.behind) return { kind: "pull" as const, title: "Pull origin", sub: `${s.behind} ${s.behind === 1 ? "commit" : "commits"} por traer${s.ahead ? `, ${s.ahead} por subir` : ""}`, icon: "arrowDown" as const };
    if (s?.ahead) return { kind: "push" as const, title: "Push origin", sub: `${s.ahead} ${s.ahead === 1 ? "commit" : "commits"} por subir`, icon: "arrowUp" as const };
    return { kind: "fetch" as const, title: "Fetch origin", sub: fetching() ? "Actualizando…" : (fetched() ? `Última vez: ${ago(fetched())}` : "Sin fetch aún"), icon: "sync" as const };
  };
  const [rejected, setRejected] = createSignal(false); // push rechazado: el remoto tiene commits que no tienes
  let quietUntil = 0; // el fetch toca FETCH_HEAD y refs: el watcher avisaría y repintaría todo por nada
  const [syncMenu, setSyncMenu] = createSignal(false);
  const [pullRebase, setPullRebaseSig] = createSignal(false);
  commands.pullRebase().then(setPullRebaseSig).catch(() => {});
  const togglePullRebase = async () => { const v = !pullRebase(); setPullRebaseSig(v); try { await commands.setPullRebase(v); } catch (e) { setPullRebaseSig(!v); setErr(String(e)); } };
  // Como GitHub Desktop: el desplegable ofrece solo lo que aplica ahora y que no sea ya el botón principal.
  const syncOpts = () => {
    const s = sync(), main = syncAct().kind;
    if (!s?.has_origin) return [];
    const o: ("fetch" | "pull" | "push")[] = [];
    if (main !== "fetch") o.push("fetch");
    if (main !== "pull" && s.behind) o.push("pull");
    if (main !== "push" && s.upstream && s.ahead) o.push("push");
    return o;
  };
  // Fetch silencioso al abrir/cambiar de repo (si el último tiene más de 5 min) y cada 10 min: así «Pull origin» aparece solo.
  const lastAuto = new Map<string, number>();
  const [autoTick, setAutoTick] = createSignal(0);
  const autoTimer = setInterval(() => setAutoTick((n) => n + 1), 600_000);
  onCleanup(() => clearInterval(autoTimer));
  createEffect(() => {
    autoTick();
    const p = cur(), st = sync(), f = fetched();
    if (!p || sync.loading || fetched.loading || !st?.has_origin || fetching()) return;
    const now = Date.now();
    if (now - (lastAuto.get(p) ?? 0) < 300_000 || (f && now / 1000 - f < 300)) return;
    lastAuto.set(p, now);
    (async () => {
      setFetching(true); setProg(undefined);
      try { setFetched(await commands.fetch(p)); quietUntil = Date.now() + 2500; } catch { /* sin red o sin permisos: se intenta de nuevo luego */ }
      setFetching(false); setProg(undefined); refSync();
    })();
  });
  // `as`: acción elegida en el desplegable; si no, la que sugiere el estado de la rama.
  async function doSync(force = false, as?: "fetch" | "pull" | "push") {
    setSyncMenu(false);
    const k = as ?? syncAct().kind;
    if (k === "publish") { setPubName(name(cur()!)); setPubDesc(""); setPubPrivate(true); setPubDlg(true); return; }
    setFetching(true); setProg(undefined); setErr(""); setRejected(false);
    try {
      // Antes de subir se comprueba el remoto: si tiene commits nuevos, no se intenta el push; el botón pasa a «Pull origin».
      let behind = false;
      if (k === "push" && !force) { setFetched(await commands.fetch(cur()!)); behind = !!(await commands.syncState(cur()!)).behind; }
      if (behind) quietUntil = Date.now() + 2500;
      else if (k === "pull") await commands.pull(cur()!);
      else if (k === "push") { await commands.push(cur()!, force); offerPr(); }
      else setFetched(await commands.fetch(cur()!));
      if (k !== "fetch") { setFetched(await commands.lastFetched(cur()!)); refetch(); refHistory(); refBranch(); }
      else quietUntil = Date.now() + 2500;
    } catch (e) {
      const m = String(e);
      if (/without .?workflow.? scope/i.test(m)) setErr("GitHub rechazó el push: tu sesión no tiene permiso para crear o modificar workflows (.github/workflows). Cierra sesión y vuelve a iniciarla para concederlo.");
      // «[rejected]» = el remoto va por delante; «[remote rejected]» es otra cosa (permisos, rama protegida, hooks) y se muestra tal cual.
      else if (k === "push" && /\[rejected\]|non-fast-forward|fetch first/.test(m)) setRejected(true); else setErr(m);
    }
    setFetching(false); setProg(undefined); refSync();
  }
  async function pullThenClose() {
    setRejected(false); setFetching(true); setProg(undefined); setErr("");
    try { await commands.pull(cur()!); refetch(); refHistory(); refBranch(); } catch (e) { setErr(String(e)); }
    setFetching(false); setProg(undefined); refSync();
  }
  // ---- Pull Requests ----
  const [prState, setPrState] = createSignal<"open" | "closed" | "all">("open");
  const [prSel, setPrSel] = createSignal<number>();
  const [prFile, setPrFile] = createSignal<string>();
  const [prBodyOpen, setPrBodyOpen] = createSignal(false);
  const [prs, { refetch: refPrs }] = createResource(
    () => (tab() === "prs" && cur() && ghUser() ? ([cur()!, prState()] as const) : undefined),
    ([p, st]) => commands.prList(p, st),
  );
  const [prd, { refetch: refPrd }] = createResource(() => (cur() && prSel() !== undefined && tab() === "prs" ? ([cur()!, prSel()!] as const) : undefined), ([p, n]) => commands.prDetail(p, n));
  createEffect(() => { prSel(); setPrBodyOpen(false); });
  createEffect(() => { const d = prd(); if (d) setPrFile(d.files[0]?.path); });
  createEffect(() => { cur(); setPrSel(undefined); setPrFile(undefined); });
  const prPatch = () => prd()?.files.find((f) => f.path === prFile())?.patch ?? null;
  const prStatusKind = (s: string) => (s === "added" ? "Added" : s === "removed" ? "Removed" : s === "renamed" ? "Rename" : "Modified");
  const isoAgo = (iso: string) => ago(Math.floor(Date.parse(iso) / 1000));
  const prBadge = (st: string, draft: boolean) => (draft && st === "open" ? "Borrador" : st === "merged" ? "Fusionado" : st === "closed" ? "Cerrado" : "Abierto");
  const [mergeMethod, setMergeMethod] = createSignal<"merge" | "squash" | "rebase">("merge");
  const [prBusy, setPrBusy] = createSignal(false);
  // ---- Publicar repo local en GitHub ----
  const [pubDlg, setPubDlg] = createSignal(false);
  const [pubName, setPubName] = createSignal("");
  const [pubDesc, setPubDesc] = createSignal("");
  const [pubPrivate, setPubPrivate] = createSignal(true); // por defecto privado: publicar por error en abierto no se deshace
  const [pubBusy, setPubBusy] = createSignal(false);
  async function doPublish() {
    setPubBusy(true); setProg(undefined); setErr("");
    try { await commands.ghPublish(cur()!, pubName().trim(), pubDesc(), pubPrivate()); setPubDlg(false); askedPriv.delete(cur()!); refSync(); reload(); }
    catch (e) { setErr(String(e)); }
    setPubBusy(false); setProg(undefined);
  }
  const [prView, setPrView] = createSignal<"files" | "talk">("files");
  createEffect(() => { prSel(); setPrView("files"); });
  const [checks] = createResource(
    () => (tab() === "prs" && cur() && prd()?.head_sha ? ([cur()!, prd()!.head_sha] as const) : undefined),
    ([p, sha]) => commands.prChecks(p, sha).catch(() => []),
  );
  const [comments, { refetch: refComments }] = createResource(() => (tab() === "prs" && cur() && prSel() !== undefined ? ([cur()!, prSel()!] as const) : undefined), ([p, n]) => commands.prComments(p, n));
  const [newComment, setNewComment] = createSignal("");
  const sendComment = (n: number) => prAct(async () => { await commands.prComment(cur()!, n, newComment()); setNewComment(""); refComments(); });
  const checksSummary = () => {
    const c = checks() ?? [];
    const bad = c.filter((x) => x.state === "failure").length, wait = c.filter((x) => x.state === "pending").length;
    return !c.length ? "" : bad ? `${bad} de ${c.length} checks fallan` : wait ? `${wait} de ${c.length} checks en curso` : `${c.length} checks correctos`;
  };
  const [mergeAsk, setMergeAsk] = createSignal(false); // fusionar y cerrar piden confirmación: no se deshacen
  const [closeAsk, setCloseAsk] = createSignal(false);
  createEffect(() => { prSel(); setMergeAsk(false); setCloseAsk(false); });
  async function prAct(f: () => Promise<unknown>) {
    setPrBusy(true); setErr("");
    try { await f(); } catch (e) { setErr(String(e)); }
    setPrBusy(false);
  }
  const prCheckout = (n: number) => prAct(async () => {
    setProg(undefined);
    await commands.prCheckout(cur()!, n);
    refBranch(); refetch(); refHistory(); refSync(); setTab("changes");
  });
  const prMerge = (n: number) => prAct(async () => { setMergeAsk(false); await commands.prMerge(cur()!, n, mergeMethod()); refPrs(); refPrd(); });
  const prClose = (n: number) => prAct(async () => { setCloseAsk(false); await commands.prClose(cur()!, n); refPrs(); refPrd(); });
  // Crear PR
  const [prNew, setPrNew] = createSignal(false);
  const [npTitle, setNpTitle] = createSignal("");
  const [npBody, setNpBody] = createSignal("");
  const [npBase, setNpBase] = createSignal("");
  const [npDraft, setNpDraft] = createSignal(false);
  const [defBranch] = createResource(() => (prNew() ? cur() : undefined), (p) => commands.defaultBranch(p).catch(() => "main"));
  const [npBranches] = createResource(() => (prNew() ? cur() : undefined), (p) => commands.branches(p));
  createEffect(() => { if (prNew() && !npBase()) setNpBase(defBranch() ?? ""); });
  const npBaseOpts = () => [...new Set((npBranches() ?? []).filter((b) => b.remote).map((b) => b.name.replace(/^origin\//, "")).filter((n) => n !== "HEAD" && n !== branch()))];
  function openNewPr() {
    setNpTitle(commits()[0]?.summary ?? ""); setNpBody(""); setNpBase(""); setNpDraft(false); setPrNew(true);
  }
  async function createPr() {
    await prAct(async () => {
      // el PR necesita la rama en origin: se publica (o se suben los commits pendientes) antes
      const s = sync();
      if (!s?.upstream || s.ahead) { setProg(undefined); await commands.push(cur()!, false); refSync(); }
      const p = await commands.prCreate(cur()!, npTitle(), npBody(), branch()!, npBase(), npDraft());
      setPrNew(false); setPrState("open"); setPrSel(p.number); refPrs();
    });
  }
  // ---- Stash ----
  const [stashMode, setStashMode] = createSignal(false); // la barra lateral muestra los stash en vez de los cambios
  const [stashEdit, setStashEdit] = createSignal<{ index: number; msg: string }>();
  const [stashSel, setStashSel] = createSignal<number>();
  const [stashFile, setStashFile] = createSignal<string>();
  const [stFiles] = createResource(() => (cur() && stashSel() !== undefined ? ([cur()!, stashSel()!] as const) : undefined), ([p, i]) => commands.stashFiles(p, i));
  const [stDiff] = createResource(() => (cur() && stashSel() !== undefined && stashFile() ? ([cur()!, stashSel()!, stashFile()!] as const) : undefined), ([p, i, f]) => commands.stashDiff(p, i, f));
  createEffect(() => { const f = stFiles(); if (f) setStashFile(f[0]?.path); });
  createEffect(() => { cur(); setStashSel(undefined); setStashFile(undefined); setStashMode(false); setStashEdit(undefined); });
  const [stashMsg, setStashMsg] = createSignal("");
  const [stashes, { refetch: refStash }] = createResource(cur, (p) => commands.stashList(p));
  const stashCur = () => stashes()?.find((x) => x.index === stashSel());
  async function stashRename() {
    const e = stashEdit();
    if (!e) return;
    setErr("");
    try { await commands.stashRename(cur()!, e.index, e.msg); setStashEdit(undefined); setStashSel(0); refStash(); }
    catch (x) { setErr(String(x)); }
  }
  const askDropStash = (st: { index: number; message: string }) =>
    setConfirmDel({ title: "¿Eliminar este stash?", body: `«${st.message}» se borra y Grove no puede deshacerlo. Si solo quieres sacar los cambios sin perderlos, usa «Aplicar».`, label: "Eliminar stash", run: () => stashDo(() => commands.stashOp(cur()!, st.index, "drop")) });
  async function stashDo(f: () => Promise<unknown>) {
    setErr("");
    try { await f(); setStashSel(undefined); refStash(); refetch(); } catch (e) { setErr(String(e)); }
  }
  async function resolve(file: string, side: "ours" | "theirs" | "manual") {
    setErr("");
    try { await commands.resolveConflict(cur()!, file, side); refetch(); } catch (e) { setErr(String(e)); }
    refSync();
  }
  async function mergeContinue() {
    setErr(""); setNote("");
    try {
      const made = await commands.mergeContinue(cur()!);
      setResolveOpen(false);
      setNote(made ? "Operación concluida." : "Sin cambios que aplicar: esos cambios ya estaban en la rama, así que se omitió el commit.");
      refetch(); refHistory();
    } catch (e) { setErr(String(e)); }
    await refSync();
    if (sync()?.merging) { setNote(""); setResolveOpen(true); } // un rebase puede detenerse de nuevo en el siguiente commit
  }
  async function abortMerge() {
    setErr(""); setResolveOpen(false);
    try { await commands.mergeAbort(cur()!); refetch(); refHistory(); } catch (e) { setErr(String(e)); }
    refSync();
  }
  // Operaciones que pueden acabar en conflicto: si pasa, `sync().merging` muestra el banner para resolver o abortar.
  const [note, setNote] = createSignal("");
  // Tras subir una rama que no es la principal, se ofrece abrir el Pull Request.
  const [prOffer, setPrOffer] = createSignal<string>();
  async function offerPr() {
    const p = cur(), b = branch();
    if (!p || !b || !ghUser() || !repos()?.find((r) => r.path === p)?.repo) return;
    try { if (b !== (await commands.defaultBranch(p))) setPrOffer(b); } catch { /* sin rama por defecto conocida: no se ofrece */ }
  }
  createEffect(on([cur, branch], () => setPrOffer(undefined), { defer: true }));
  const [resolveOpen, setResolveOpen] = createSignal(false);
  const [previewFile, setPreviewFile] = createSignal<string>();
  const [sides] = createResource(() => (resolveOpen() && previewFile() ? ([cur()!, previewFile()!] as const) : undefined), ([p, f]) => commands.conflictSides(p, f));
  createEffect(() => { if (!resolveOpen()) setPreviewFile(undefined); });
  const [helpOpen, setHelpOpen] = createSignal(false);
  const [helpSel, setHelpSel] = createSignal(TOPICS[0].id);
  // Cherry-pick y revert explican qué van a hacer antes de la primera vez (se puede silenciar).
  const [opAsk, setOpAsk] = createSignal<{ kind: "pick" | "revert"; id: string; from?: string }>();
  const skipKey = "grove.skipOpAsk";
  const skipAsk = () => { try { return localStorage.getItem(skipKey) === "1"; } catch { return false; } };
  const askOp = (kind: "pick" | "revert", id: string) => {
    setCctx(undefined);
    if (skipAsk()) kind === "pick" ? doPick(id) : doRevert(id); else setOpAsk({ kind, id, from: kind === "pick" ? fRev() : undefined });
  };
  // El diálogo de conflictos se abre solo al empezar una operación a medias (y se cierra al terminar).
  createEffect(on(() => !!sync()?.merging, (m) => setResolveOpen(m)));
  const [mergeBr, setMergeBr] = createSignal<string>();
  const [tags, { refetch: refTags }] = createResource(() => (tab() === "history" ? cur() : undefined), (p) => commands.tags(p));
  const tagsOf = (id: string) => (tags() ?? []).filter((t) => t.id.slice(0, 7) === id.slice(0, 7));
  const [tagDlg, setTagDlg] = createSignal<{ id: string; name: string; msg: string; push: boolean }>();
  const [tagBusy, setTagBusy] = createSignal(false);
  async function createTag() {
    const d = tagDlg();
    if (!d || !d.name.trim() || tagBusy()) return;
    setTagBusy(true); setErr("");
    try { await commands.createTag(cur()!, d.id, d.name, d.msg, d.push); setTagDlg(undefined); setNote(`Tag «${d.name.trim()}» creado${d.push ? " y subido" : ""}.`); refTags(); }
    catch (e) { setErr(String(e)); }
    setTagBusy(false);
  }
  const [cctx, setCctx] = createSignal<{ x: number; y: number; id: string; summary: string }>();
  async function gitOp(f: () => Promise<boolean>, okMsg: string) {
    setCctx(undefined); setMergeBr(undefined); setMenu(null); setErr(""); setNote("");
    try { setNote((await f()) ? okMsg : "Hay conflictos: resuélvelos abajo o aborta la operación."); } catch (e) { setErr(String(e)); }
    refetch(); refHistory(); refBranch(); refSync(); refBranches();
  }
  const doMerge = (b: string) => gitOp(() => commands.mergeBranch(cur()!, b), `«${b}» fusionada en «${branch()}».`);
  const doRevert = (id: string) => gitOp(() => commands.revertCommit(cur()!, id), `Commit ${id} revertido.`);
  // ---- Crear rama en un commit / volver a un commit / recuperar (reflog) ----
  const [branchAt, setBranchAt] = createSignal<{ rev: string; name: string; sw: boolean }>();
  const [resetDlg, setResetDlg] = createSignal<{ id: string; summary: string; mode: "mixed" | "hard"; n: number | null }>();
  const [reflogOpen, setReflogOpen] = createSignal(false);
  // ---- Comparar ramas ----
  const [cmpOpen, setCmpOpen] = createSignal(false);
  const [cmpBase, setCmpBase] = createSignal("");
  const [cmpOther, setCmpOther] = createSignal("");
  const [cmpFile, setCmpFile] = createSignal<string>();
  const [cmpBranches] = createResource(() => (cmpOpen() ? cur() : undefined), (p) => commands.branches(p));
  const [cmp] = createResource(() => (cmpOpen() && cmpBase() && cmpOther() && cmpBase() !== cmpOther() ? ([cur()!, cmpBase(), cmpOther()] as const) : undefined), ([p, a, b]) => commands.compareBranches(p, a, b));
  const [cmpDiff] = createResource(() => (cmpFile() && cmp() ? ([cur()!, cmpBase(), cmpOther(), cmpFile()!] as const) : undefined), ([p, a, b, f]) => commands.compareDiff(p, a, b, f));
  createEffect(() => { const c = cmp(); setCmpFile(c?.files[0]?.path); });
  const openCompare = (other = "") => { setMenu(null); setCmpBase(branch() ?? ""); setCmpOther(other); setCmpOpen(true); };
  const [reflog, { refetch: refReflog }] = createResource(() => (reflogOpen() ? cur() : undefined), (p) => commands.reflog(p, 100));
  const [opBusy, setOpBusy] = createSignal(false);
  async function doBranchAt() {
    const d = branchAt();
    if (!d || !d.name.trim() || opBusy()) return;
    setOpBusy(true); setErr("");
    try { await commands.branchAt(cur()!, d.name, d.rev, d.sw); setBranchAt(undefined); setNote(`Rama «${d.name.trim()}» creada en ${d.rev}${d.sw ? " y activa" : ""}.`); refBranch(); if (d.sw) { refetch(); refHistory(); } }
    catch (e) { setErr(String(e)); }
    setOpBusy(false);
  }
  async function openReset(c: { id: string; summary: string }) {
    setCctx(undefined);
    setResetDlg({ id: c.id, summary: c.summary, mode: "mixed", n: null });
    try { const n = await commands.commitsSince(cur()!, c.id); setResetDlg((d) => (d ? { ...d, n } : d)); } catch (e) { setResetDlg(undefined); setErr(String(e)); }
  }
  async function doReset() {
    const d = resetDlg();
    if (!d || opBusy()) return;
    setOpBusy(true); setErr("");
    try { await commands.resetTo(cur()!, d.id, d.mode); setResetDlg(undefined); setNote(d.mode === "hard" ? `Rama devuelta a ${d.id}; cambios descartados. Los commits quitados siguen en «Recuperar».` : `Rama devuelta a ${d.id}; los cambios quitados están en Cambios.`); }
    catch (e) { setErr(String(e)); }
    setOpBusy(false);
    refetch(); refHistory(); refBranch(); refSync(); if (reflogOpen()) refReflog();
  }
  const doPick = (id: string) => gitOp(() => commands.cherryPick(cur()!, id), `Commit ${id} aplicado en «${branch()}».`);
  // Fetch + avance rápido seguro de todo un grupo, en paralelo en el backend; cada repo actualiza su indicador al terminar.
  const [syncingGrp, setSyncingGrp] = createSignal<string>();
  async function syncGroup(group: string, paths: string[]) {
    if (syncingGrp() || !paths.length) return;
    setSyncingGrp(group); setErr(""); setNote("");
    try {
      const rs = await commands.syncRepos(paths, true);
      const n = (s: string) => rs.filter((r) => r.status === s);
      const bad = n("failed"), behind = n("behind"), up = n("updated");
      const parts = [up.length && `${up.length} actualizados`, behind.length && `${behind.length} con commits por traer a mano (cambios o rama divergida)`, bad.length && `${bad.length} con error (${bad.slice(0, 3).map((r) => label(r.path)).join(", ")}${bad.length > 3 ? "…" : ""})`].filter(Boolean);
      setNote(`Grupo «${group}»: ${parts.length ? parts.join(" · ") : `${rs.length} al día`}.`);
    } catch (e) { setErr(String(e)); }
    setSyncingGrp(undefined);
    if (cur()) { refSync(); refetch(); refHistory(); refBranch(); setFetched(await commands.lastFetched(cur()!)); }
  }
  async function switchTo(b: string, mode: "Plain" | "Stash" | "Carry" = "Plain") {
    setMenu(null); setBlocked(undefined); setErr("");
    try {
      if (!(await commands.switchBranch(cur()!, b, mode))) { setBlocked(b); return; }
    } catch (e) { setErr(String(e)); }
    refBranch(); refetch(); refHistory(); setFile(undefined);
  }
  async function pick(p: string) {
    setMenu(null); setFile(undefined); setErr("");
    try { await commands.addRepo(p); await reload(); setCurrent(p); } catch (e) { setErr(String(e)); }
  }
  async function add() {
    const p = await open({ directory: true, title: "Añadir repositorio local" });
    if (typeof p === "string") await pick(p);
  }
  // ---- Clonar ----
  const DIR_KEY = "grove.cloneDir";
  const [cloning, setCloning] = createSignal<"gh" | "url" | null>(null);
  const [cloneFilter, setCloneFilter] = createSignal("");
  const [cloneUrl, setCloneUrl] = createSignal("");
  const [cloneName, setCloneName] = createSignal("");
  const [cloneDir, setCloneDir] = createSignal((() => { try { return localStorage.getItem(DIR_KEY) ?? ""; } catch { return ""; } })());
  const [cloneBusy, setCloneBusy] = createSignal(false);
  const [ghRepos] = createResource(() => (cloning() === "gh" && ghUser() ? "x" : undefined), () => commands.ghRepos());
  const ghShown = () => (ghRepos() ?? []).filter((r) => r.full_name.toLowerCase().includes(cloneFilter().toLowerCase()));
  // nombre de carpeta a partir de la URL: .../owner/repo.git -> repo
  const nameFromUrl = (u: string) => (u.trim().replace(/[\\/]+$/, "").replace(/\.git$/, "").split(/[\\/:]/).pop() ?? "");
  function openClone() { setMenu(null); setCloning(ghUser() ? "gh" : "url"); setCloneUrl(""); setCloneName(""); setCloneFilter(""); }
  async function chooseDir() {
    const d = await open({ directory: true, title: "Carpeta donde clonar" });
    if (typeof d === "string") { setCloneDir(d); try { localStorage.setItem(DIR_KEY, d); } catch { /* solo esta sesión */ } }
  }
  async function doClone() {
    setCloneBusy(true); setProg(undefined); setErr("");
    try {
      const dest = await commands.cloneRepo(cloneDir(), cloneUrl(), cloneName());
      await reload(); setCurrent(dest); setCloning(null);
    } catch (e) { setErr(String(e)); }
    setCloneBusy(false); setProg(undefined);
  }
  async function remove(p: string) {
    await commands.removeRepo(p);
    if (current() === p) setCurrent(undefined);
    await reload();
  }
  const [onlyNews, setOnlyNews] = createSignal(false);
  const hasNews = (path: string) => { const o = ov()?.find((x) => x.path === path); return !!o && (o.changes > 0 || o.ahead > 0 || o.behind > 0); };
  const newsCount = () => (repos() ?? []).filter((r) => hasNews(r.path)).length;
  const shown = () => (repos() ?? []).filter((r) => r.path.toLowerCase().includes(filter().toLowerCase()) && (!onlyNews() || hasNews(r.path)));
  // Como GitHub Desktop: "Recientes" arriba y luego agrupado por dueño del remoto.
  const label = (path: string) => { const r = repos()?.find((x) => x.path === path); return r ? repoLabel(r) : name(path); };
  // "Recientes" solo cuando la lista es larga; con pocos repos se ve directamente por grupo.
  // Se vuelve a pedir cada vez que cambia la lista de repos; incluye los grupos vacíos creados a mano.
  const [groupList] = createResource(() => repos(), () => commands.listGroups());
  const knownGroups = () => groupList() ?? [];
  const groups = createMemo(() => {
    // Siempre alfabético por el nombre que se ve; elegir un repo nunca cambia el orden (sin «Recientes»).
    const all = [...shown()].sort((a, b) => repoLabel(a).localeCompare(repoLabel(b), "es", { sensitivity: "base", numeric: true }));
    const nRecent = 0;
    const g = new Map<string, { custom: boolean; list: typeof all }>();
    for (const r of all.slice(nRecent)) {
      const k = r.group || r.owner || "Otros";
      const e = g.get(k) ?? { custom: !!r.group, list: [] };
      e.list.push(r);
      g.set(k, e);
    }
    if (!filter()) for (const n of knownGroups()) if (!g.has(n)) g.set(n, { custom: true, list: [] });
    // grupos propios primero, luego dueños del remoto, "Otros" al final
    const rank = ([k, v]: [string, { custom: boolean }]) => (v.custom ? 0 : k === "Otros" ? 2 : 1);
    const sorted = [...g.entries()].sort((a, b) => rank(a) - rank(b) || a[0].localeCompare(b[0]));
    return { recent: all.slice(0, nRecent), sorted };
  });
  const [editRepo, setEditRepo] = createSignal<{ path: string; alias: string; group: string }>();
  const [addOpen, setAddOpen] = createSignal(false);
  const [newGroupOpen, setNewGroupOpen] = createSignal(false);
  const [groupName, setGroupName] = createSignal("");
  async function createGroup() {
    const n = groupName().trim();
    if (!n) return;
    setErr("");
    try { await commands.addGroup(n); setGroupName(""); setNewGroupOpen(false); await reload(); } catch (x) { setErr(String(x)); }
  }
  async function deleteGroup(n: string) {
    setErr("");
    try { await commands.deleteGroup(n); await reload(); } catch (x) { setErr(String(x)); }
  }
  // Añadir un repo existente directamente a un grupo.
  async function addTo(group: string) {
    const p = await open({ directory: true, title: `Añadir repositorio a «${group}»` });
    if (typeof p === "string") { await pick(p); await setGroup(p, group); }
  }
  async function saveMeta() {
    const e = editRepo()!;
    setErr("");
    try { await commands.setRepoMeta(e.path, e.alias, e.group); setEditRepo(undefined); await reload(); } catch (x) { setErr(String(x)); }
  }

  const menuLeft = () => (menu() === "repo" ? 0 : menu() === "wt" ? side() : side() + 230);
  // Se decide con el último dato conocido, también mientras carga: así al cambiar de repo no parpadea «Selecciona un archivo» antes de «No hay cambios».
  const emptyChanges = () => !!changes.latest && !changes.latest.length;
  // path -> privado (true) / público (false); ausente = no se sabe (no es GitHub o sin red)
  const [priv, setPriv] = createSignal<Record<string, boolean>>({});
  const askedPriv = new Map<string, boolean>(); // path -> ¿se preguntó con sesión?
  createEffect(() => {
    const logged = !!ghUser(); // sin token los privados dan 404 y quedan "desconocidos": se reintenta al iniciar sesión
    for (const r of repos() ?? []) {
      if (askedPriv.get(r.path) === logged) continue;
      askedPriv.set(r.path, logged);
      commands.repoPrivate(r.path).then((v) => v !== null && setPriv((m) => ({ ...m, [r.path]: v }))).catch(() => {});
    }
  });
  const repoIcon = (path: string) => (priv()[path] ? "lock" : "repo");
  // Indicadores por repo (como GitHub Desktop): punto = cambios sin commitear, ↑ = commits por subir, ↓ = por traer.
  const [ov, { refetch: refOv, mutate: setOv }] = createResource(() => repos()?.map((r) => r.path), (ps) => commands.reposOverview(ps));
  // El backend refresca (y hace fetch de) todos los repos al abrir/volver a la app y emite el resultado de cada uno.
  const unov = listen<RepoOverview>("repo-overview", (e) => setOv((l) => [...(l ?? []).filter((o) => o.path !== e.payload.path), e.payload]));
  onCleanup(() => unov.then((f) => f()));
  createEffect(on([sync, changes], () => refOv(), { defer: true }));
  const ovOf = (path: string) => ov()?.find((o) => o.path === path);
  const [moving, setMoving] = createSignal<string>();
  const [ctx, setCtx] = createSignal<{ x: number; y: number; path: string }>();
  const [infoPath, setInfoPath] = createSignal<string>();
  const [info] = createResource(infoPath, (p) => commands.repoInfo(p));
  const ctxDo = async (f: () => unknown) => { setCtx(undefined); try { await f(); } catch (x) { setErr(String(x)); } };
  const [newGroup, setNewGroup] = createSignal("");
  const [dragOver, setDragOver] = createSignal<string>();
  // Cerrar el panel de repositorios (clic fuera, Escape, elegir repo) limpia también lo que quedó abierto dentro.
  createEffect(() => { if (menu() !== "repo") { setCtx(undefined); setNewGroupOpen(false); setAddOpen(false); setMoving(undefined); } });
  async function setGroup(path: string, group: string) {
    const r = repos()?.find((x) => x.path === path);
    if (!r || r.group === group.trim()) return;
    setErr("");
    try { await commands.setRepoMeta(path, r.alias, group.trim()); setMoving(undefined); setNewGroup(""); await reload(); } catch (x) { setErr(String(x)); }
  }
  const RepoItem = (p: { r: { path: string; alias: string; group: string; repo?: string | null } }) => (<>
    <div class="li fo" classList={{ sel: p.r.path === cur() }} onClick={() => pick(p.r.path)} draggable={true}
      onContextMenu={(e) => { e.preventDefault(); setMoving(undefined); setCtx({ x: Math.min(e.clientX, window.innerWidth - 230), y: Math.min(e.clientY, window.innerHeight - 330), path: p.r.path }); }}
      onDragStart={(e) => { e.dataTransfer?.setData("text/plain", p.r.path); setMoving(undefined); }} onDragEnd={() => setDragOver(undefined)}>
      <Icon name={repoIcon(p.r.path)} />
      <span class="grow ellip" title={`${p.r.path}${p.r.repo && p.r.repo !== name(p.r.path) ? `\nGitHub: ${p.r.repo}` : ""}`}>{repoLabel(p.r)}</span>
      <Show when={ovOf(p.r.path)}>
        {(o) => (<span class="rind">
          <Show when={o().ahead}><span class="rbadge" title={`${o().ahead} commits por subir`}><Icon name="arrowUp" size={11} />{o().ahead}</span></Show>
          <Show when={o().behind}><span class="rbadge" title={`${o().behind} commits por traer`}><Icon name="arrowDown" size={11} />{o().behind}</span></Show>
          <Show when={o().changes}><span class="rdot" title={`${o().changes} archivos con cambios`} /></Show>
        </span>)}
      </Show>
    </div>
    <Show when={moving() === p.r.path}>
      <div class="gmove" onClick={(e) => e.stopPropagation()}>
        <For each={knownGroups()}>{(g) => <button class="chip" classList={{ on: g === p.r.group }} onClick={() => setGroup(p.r.path, g)}>{g}</button>}</For>
        <Show when={p.r.group}><button class="chip out" onClick={() => setGroup(p.r.path, "")}>Sin grupo</button></Show>
        <div class="fsearch"><input placeholder="Nuevo grupo…" value={newGroup()} onInput={(e) => setNewGroup(e.currentTarget.value)} onKeyDown={(e) => e.key === "Enter" && newGroup().trim() && setGroup(p.r.path, newGroup())} /></div>
      </div>
    </Show>
  </>);
  const [renaming, setRenaming] = createSignal<string>();
  const afterBranchOp = () => { setRenaming(undefined); setNewBranch(""); refBranch(); refBranches(); refStale(); refHistBranches(); refetch(); refHistory(); };
  async function renameBranch() {
    setErr("");
    try { await commands.renameBranch(cur()!, renaming()!, newBranch()); afterBranchOp(); } catch (e) { setErr(String(e)); }
  }
  // Primer intento con -d (git rechaza si no está fusionada); si falla por eso, se pregunta de nuevo avisando de la pérdida.
  async function deleteBranch(name: string, force: boolean) {
    setErr("");
    try { await commands.deleteBranch(cur()!, name, force); afterBranchOp(); }
    catch (e) {
      const m = String(e);
      if (!force && /not fully merged/i.test(m)) setConfirmDel({ title: `«${name}» tiene commits sin fusionar`, body: "Esos commits no están en ninguna otra rama: si la borras se pierden. Mientras git los conserve, podrás recuperarlos desde «Recuperar», en el Historial.", label: "Borrar y perder commits", run: () => deleteBranch(name, true) });
      else setErr(m);
    }
  }
  const askDeleteBranch = (name: string) =>
    setConfirmDel({ title: `¿Borrar la rama «${name}»?`, body: "Se borra solo en tu equipo, no en GitHub. Si tiene commits que no están en otra rama, te avisaremos antes de continuar.", label: "Borrar rama", run: () => deleteBranch(name, false) });
  const BranchItem = (p: { b: { name: string; time: number; remote: boolean } }) => (
    <div class="li fo" classList={{ sel: p.b.name === branch() }} onClick={() => switchTo(p.b.name)}>
      {p.b.name === branch() ? <Icon name="check" /> : <Icon name="gitBranch" />}
      <span class="grow ellip" title={p.b.name}>{p.b.name}</span>
      <Show when={!p.b.remote && staleOf(p.b.name)} keyed>{(st) => <span class="tagb warn" title={st.merged ? "Su rama en GitHub ya no existe y sus commits están en otra rama: se puede borrar sin perder nada" : "Su rama en GitHub ya no existe y tiene commits que no están en ninguna otra rama"}>{st.merged ? "remota eliminada" : "sin fusionar"}</span>}</Show>
      <Show when={!p.b.remote} fallback={<><small class="when">{ago(p.b.time)}</small><button class="ibtn" title={`Fusionar «${p.b.name}» en «${branch()}»`} onClick={(e) => { e.stopPropagation(); setMergeBr(p.b.name); }}><Icon name="gitMerge" size={14} /></button><button class="ibtn danger" title="Borrar la rama en origin" onClick={(e) => { e.stopPropagation(); setDelRemote(p.b.name); }}><Icon name="trash" size={14} /></button></>}>
        <small class="when">{ago(p.b.time)}</small>
        <button class="ibtn" title="Renombrar" onClick={(e) => { e.stopPropagation(); setRenaming(p.b.name); setNewBranch(p.b.name); }}><Icon name="pencil" size={14} /></button>
        <Show when={p.b.name !== branch()}>
          <button class="ibtn" title={`Fusionar «${p.b.name}» en «${branch()}»`} onClick={(e) => { e.stopPropagation(); setMergeBr(p.b.name); }}><Icon name="gitMerge" size={14} /></button>
          <button class="ibtn danger" title="Borrar" onClick={(e) => { e.stopPropagation(); askDeleteBranch(p.b.name); }}><Icon name="trash" size={14} /></button>
        </Show>
      </Show>
    </div>
  );

  // ---- Paleta de comandos (⌘/Ctrl+K): saltar a un repo o rama y lanzar acciones escribiendo unas letras ----
  const [palOpen, setPalOpen] = createSignal(false);
  const [palQ, setPalQ] = createSignal("");
  const [palIdx, setPalIdx] = createSignal(0);
  const [palBranches] = createResource(() => (palOpen() ? cur() : undefined), (p) => commands.branches(p));
  const openPal = () => { setPalQ(""); setPalIdx(0); setPalOpen(true); };
  const palItems = createMemo<PalItem[]>(() => {
    if (!palOpen()) return [];
    const q = palQ().trim(), has = !!cur();
    const go = (t: "changes" | "history" | "prs") => () => setTab(t);
    const actions: PalItem[] = [
      ...(has ? [
        { id: "a-sync", group: "Acciones", icon: "sync", label: "Sincronizar (fetch, pull o push según el estado)", hint: "⌘⇧P", run: () => doSync() },
        { id: "a-fetch", group: "Acciones", icon: "sync", label: "Fetch origin", hint: "⌘⇧F", run: () => doSync(false, "fetch") },
        { id: "a-branch", group: "Acciones", icon: "gitBranch", label: "Nueva rama…", run: () => { setNewBranch(""); setCreating(true); } },
        { id: "a-changes", group: "Acciones", icon: "diffModified", label: "Ir a Cambios", hint: "⌘1", run: go("changes") },
        { id: "a-history", group: "Acciones", icon: "history", label: "Ir a Historial", hint: "⌘2", run: go("history") },
        { id: "a-prs", group: "Acciones", icon: "gitMerge", label: "Ir a Pull Requests", hint: "⌘3", run: go("prs") },
        { id: "a-stash", group: "Acciones", icon: "archive", label: "Ver stashes", run: () => { setTab("changes"); setStashSel(undefined); setStashEdit(undefined); refStash(); setStashMode(true); } },
        { id: "a-compare", group: "Acciones", icon: "gitMerge", label: "Comparar ramas…", run: () => openCompare() },
        { id: "a-reflog", group: "Acciones", icon: "history", label: "Recuperar commits perdidos (reflog)…", run: () => { setTab("history"); setReflogOpen(true); } },
        { id: "a-wt", group: "Acciones", icon: "fileDirectory", label: "Nuevo worktree…", run: openWt },
        { id: "a-editor", group: "Acciones", icon: "code", label: "Abrir en el editor", run: () => launch("Editor") },
        { id: "a-term", group: "Acciones", icon: "terminal", label: "Abrir en la terminal", run: () => launch("Terminal") },
        { id: "a-folder", group: "Acciones", icon: "fileDirectory", label: isMac ? "Mostrar en Finder" : "Mostrar en el explorador", run: () => launch("Folder") },
      ] as PalItem[] : []),
      { id: "a-all", group: "Acciones", icon: "sync", label: "Actualizar todos los repositorios", run: () => syncGroup("Todos", (repos() ?? []).map((r) => r.path)) },
      { id: "a-news", group: "Acciones", icon: "filter", label: onlyNews() ? "Mostrar todos los repositorios" : "Mostrar solo repos con novedades", run: () => { setOnlyNews(!onlyNews()); setMenu("repo"); } },
      { id: "a-help", group: "Acciones", icon: "help", label: "Ayuda", run: () => setHelpOpen(true) },
    ];
    const repoItems: PalItem[] = (repos() ?? []).map((r) => {
      const o = ov()?.find((x) => x.path === r.path);
      const hint = [o?.changes ? `● ${o.changes}` : "", o?.ahead ? `↑${o.ahead}` : "", o?.behind ? `↓${o.behind}` : "", r.group || r.owner || ""].filter(Boolean).join("  ");
      return { id: `r-${r.path}`, group: "Repositorios", icon: repoIcon(r.path) as IconName, label: repoLabel(r), hint, run: () => { setMenu(null); setFile(undefined); setCurrent(r.path); } };
    }).filter((i) => i.id !== `r-${cur()}`);
    const branchItems: PalItem[] = (palBranches() ?? []).filter((b) => b.name !== branch()).map((b) => ({ id: `b-${b.name}`, group: `Ramas de «${label(cur() ?? "")}»`, icon: "gitBranch" as IconName, label: b.name, hint: b.remote ? "remota" : "", run: () => switchTo(b.name) }));
    const rank = (list: PalItem[], max: number) => (q
      ? list.map((i) => [fuzzy(q, i.label + " " + (i.hint ?? "")), i] as const).filter(([sc]) => sc >= 0).sort((a, b) => b[0] - a[0]).slice(0, max).map(([, i]) => i)
      : list.slice(0, max));
    const news = (it: PalItem) => (it.hint?.match(/[●↑↓]/) ? 0 : 1); // sin búsqueda: primero los repos con novedades
    return [...rank(actions, q ? 6 : 8), ...rank(q ? repoItems : [...repoItems].sort((a, b) => news(a) - news(b)), 8), ...rank(branchItems, 8)];
  });
  const palRun = (it?: PalItem) => { if (!it) return; setPalOpen(false); it.run(); };
  createEffect(() => { palQ(); setPalIdx(0); });
  const palKey = (e: KeyboardEvent) => {
    const n = palItems().length;
    if (e.key === "ArrowDown") { setPalIdx((i) => (n ? (i + 1) % n : 0)); e.preventDefault(); }
    else if (e.key === "ArrowUp") { setPalIdx((i) => (n ? (i - 1 + n) % n : 0)); e.preventDefault(); }
    else if (e.key === "Enter") { palRun(palItems()[palIdx()]); e.preventDefault(); }
  };

  return (
    <div class="app" style={{ "--side": `${side()}px` }}>
      <header class="toolbar">
        <button class="tbtn repo" classList={{ open: menu() === "repo" }} onClick={() => setMenu(menu() === "repo" ? null : "repo")}>
          <Icon name={cur() ? repoIcon(cur()!) : "repo"} size={20} />
          <span class="t"><small>Repositorio actual</small><b>{cur() ? label(cur()!) : "Ninguno"}</b></span>
          <Icon name={menu() === "repo" ? "triangleUp" : "triangleDown"} class="chev" />
        </button>
        <button class="tbtn wt" classList={{ open: menu() === "wt" }} disabled={!cur()} onClick={() => setMenu(menu() === "wt" ? null : "wt")}>
          <Icon name="fileDirectory" size={20} />
          <span class="t"><small>Worktree actual</small><b>{cur() ? name(cur()!) : "—"}</b></span>
          <Icon name={menu() === "wt" ? "triangleUp" : "triangleDown"} class="chev" />
        </button>
        <button class="tbtn branch" classList={{ open: menu() === "branch" }} disabled={!cur()} onClick={() => setMenu(menu() === "branch" ? null : "branch")}>
          <Icon name="gitBranch" size={20} />
          <span class="t"><small>Rama actual</small><b>{branch() ?? "—"}</b></span>
          <Icon name={menu() === "branch" ? "triangleUp" : "triangleDown"} class="chev" />
        </button>
        <div class="tbsplit">
          <button class="tbtn fetch" disabled={!cur() || fetching()} onClick={() => doSync()}>
            <span classList={{ spin: fetching() }}><Icon name={fetching() ? "sync" : syncAct().icon} size={20} /></span>
            <span class="t"><b>{syncAct().title}</b><small>{fetching() ? (prog() ? `${prog()!.label} ${prog()!.pct}%` : "Trabajando…") : syncAct().sub}</small></span>
            <Show when={!fetching() && (sync()?.ahead || sync()?.behind)}>
              <span class="sbadge">{sync()!.ahead ? `${sync()!.ahead}↑` : ""}{sync()!.ahead && sync()!.behind ? " " : ""}{sync()!.behind ? `${sync()!.behind}↓` : ""}</span>
            </Show>
          </button>
          <Show when={sync()?.has_origin}>
            <button class="tbtn fmore" title="Más acciones" disabled={!cur() || fetching()} onClick={() => setSyncMenu(!syncMenu())}><Icon name="triangleDown" size={14} /></button>
          </Show>
          <Show when={syncMenu() && sync()?.has_origin}>
            <div class="scrim" onClick={() => setSyncMenu(false)} />
            <div class="pop sync">
              <For each={syncOpts()}>
                {(k) => (
                  <button onClick={() => doSync(false, k)}>
                    <b>{k === "fetch" ? "Fetch origin" : k === "pull" ? "Pull origin" : "Push origin"}</b>
                    <small>{k === "fetch" ? "Trae novedades sin tocar tu rama" : k === "pull" ? (pullRebase() ? "Trae y reaplica tus commits encima (rebase)" : "Trae y fusiona los commits del remoto") : "Sube tus commits a GitHub"}</small>
                  </button>
                )}
              </For>
              <label class="syncopt" title="Con rebase tus commits locales se reaplican encima de los del remoto: el historial queda lineal, sin commits de fusión."><input type="checkbox" checked={pullRebase()} onChange={togglePullRebase} /><span><b>Pull con rebase</b><small>Historial lineal, sin commits de fusión</small></span></label>
            </div>
          </Show>
        </div>
        <button class="tbtn stash" classList={{ open: stashMode() }} disabled={!cur()} onClick={() => { setTab("changes"); setStashSel(undefined); setStashEdit(undefined); refStash(); setStashMode(!stashMode()); }}>
          <Icon name="archive" size={20} />
          <span class="t"><b>Stash</b><small>{stashes()?.length ? `${stashes()!.length} ${stashes()!.length === 1 ? "guardado" : "guardados"}` : "Sin guardados"}</small></span>
        </button>
        <button class="tbtn help" title="Ayuda: qué hace cada cosa" aria-label="Ayuda" onClick={() => setHelpOpen(true)}><Icon name="help" size={20} /></button>
      </header>

      <Show when={menu()}>
        <div class="scrim" onClick={() => { setMenu(null); setCreating(false); setRenaming(undefined); }} />
        <div class="foldout" classList={{ wide: menu() === "branch" }} style={{ left: `${menuLeft()}px`, width: menu() === "branch" ? "365px" : `${menu() === "repo" ? side() : 330}px` }}>
          <Show when={menu() === "repo"}>
            <div class="ftop">
              <div class="fsearch"><Icon name="search" /><input placeholder="Filtrar" value={filter()} onInput={(e) => setFilter(e.currentTarget.value)} /></div>
              <button class="btn news" classList={{ on: onlyNews() }} title="Mostrar solo repos con cambios sin commitear, commits por subir o por traer" onClick={() => setOnlyNews(!onlyNews())}>Novedades<Show when={newsCount()}><b>{newsCount()}</b></Show></button>
              <div class="addwrap">
                <button class="btn" onClick={() => setAddOpen(!addOpen())}>Añadir <Icon name="triangleDown" size={14} /></button>
                <Show when={addOpen()}>
                  <div class="scrim" onClick={() => setAddOpen(false)} />
                  <div class="pop">
                    <button onClick={() => { setAddOpen(false); add(); }}>Añadir repositorio existente…</button>
                    <button onClick={() => { setAddOpen(false); openClone(); }}>Clonar repositorio…</button>
                    <button onClick={() => { setAddOpen(false); setGroupName(""); setNewGroupOpen(true); }}>Crear grupo…</button>
                  </div>
                </Show>
              </div>
            </div>
            <Show when={newGroupOpen()}>
              <div class="ftop gnew">
                <div class="fsearch"><Icon name="fileDirectory" /><input autofocus placeholder="Nombre del grupo" value={groupName()} onInput={(e) => setGroupName(e.currentTarget.value)}
                  onKeyDown={(e) => { if (e.key === "Enter") createGroup(); if (e.key === "Escape") { e.stopPropagation(); setNewGroupOpen(false); } }} /></div>
                <button class="btn primary" disabled={!groupName().trim()} onClick={createGroup}>Crear</button>
              </div>
            </Show>
            <div class="flist">
              <Show when={groups().recent.length}>
                <div class="fhead">Recientes</div>
                <For each={groups().recent}>{(r) => <RepoItem r={r} />}</For>
              </Show>
              <For each={groups().sorted}>
                {([group, v]) => (<><div class="fhead" classList={{ over: dragOver() === group }} title="Suelta un repositorio aquí para moverlo a este grupo"
                  onDragOver={(e) => { e.preventDefault(); setDragOver(group); }} onDragLeave={() => setDragOver(undefined)}
                  onDrop={(e) => { e.preventDefault(); setDragOver(undefined); const p = e.dataTransfer?.getData("text/plain"); if (p) setGroup(p, v.custom ? group : ""); }}>
                  <span class="grow ellip">{group}</span>
                  <Show when={v.list.length}>
                    <button class="ibtn" classList={{ spin: syncingGrp() === group }} disabled={!!syncingGrp()} title="Actualizar todo el grupo: fetch y avance rápido seguro (sin fusiones ni conflictos)"
                      onClick={() => syncGroup(group, v.list.map((r) => r.path))}><Icon name="sync" size={14} /></button>
                  </Show>
                  <Show when={v.custom}>
                    <button class="ibtn" title="Añadir un repositorio a este grupo" onClick={() => addTo(group)}><Icon name="plus" size={14} /></button>
                    <Show when={!v.list.length}><button class="ibtn" title="Borrar grupo vacío" onClick={() => deleteGroup(group)}><Icon name="trash" size={14} /></button></Show>
                  </Show>
                </div><For each={v.list}>{(r) => <RepoItem r={r} />}</For></>)}
              </For>
              <Show when={!shown().length}><p class="muted">Sin repositorios</p></Show>
            </div>
            <div class="fopt acct">
              <Show when={ghUser()} fallback={<button class="btn" onClick={ghLogin} disabled={!!ghCode()}>Iniciar sesión con GitHub</button>}>
                {(u) => (<><img class="av" src={u().avatar_url} alt="" /><b class="grow ellip">{u().login}</b><button class="btn" onClick={ghLogout}>Cerrar sesión</button></>)}
              </Show>
            </div>
          </Show>
          <Show when={menu() === "branch"}>
            <div class="ftabs"><span class="on">Ramas</span></div>
            <Show
              when={!renaming()}
              fallback={
                <div class="ftop">
                  <div class="fsearch"><Icon name="gitBranch" /><input autofocus placeholder="Nombre de la rama" value={newBranch()} onInput={(e) => setNewBranch(e.currentTarget.value)} onKeyDown={(e) => e.key === "Enter" && (renaming() ? renameBranch() : createBranch())} /></div>
                  <button class="btn primary" disabled={!newBranch().trim()} onClick={() => (renaming() ? renameBranch() : createBranch())}>{renaming() ? "Renombrar" : "Crear"}</button>

                </div>
              }
            >
              <div class="ftop">
                <div class="fsearch"><Icon name="search" /><input placeholder="Filtrar" value={filter()} onInput={(e) => setFilter(e.currentTarget.value)} /></div>
                <button class="btn" onClick={() => { setMenu(null); setNewBranch(""); setCreating(true); }}>Nueva rama</button>
              </div>
            </Show>
            <div class="flist">
              <Show when={branchGroups().def}>{(b) => (<><div class="fhead">Rama por defecto</div><BranchItem b={b()} /></>)}</Show>
              <Show when={branchGroups().recent.length}>
                <div class="fhead">Ramas recientes</div>
                <For each={branchGroups().recent}>{(b) => <BranchItem b={b} />}</For>
              </Show>
              <Show when={branchGroups().other.length}>
                <div class="fhead">Otras ramas</div>
                <For each={branchGroups().other}>{(b) => <BranchItem b={b} />}</For>
              </Show>
              <Show when={branches() && !branches()!.length}><p class="muted">Sin ramas</p></Show>
            </div>
            <div class="staleBar"><span>¿Qué le falta a una rama?</span><button class="btn sm" onClick={() => openCompare()}>Comparar ramas…</button></div>
            <Show when={stale()?.length}>
              <div class="staleBar"><span>{stale()!.length} {stale()!.length === 1 ? "rama obsoleta" : "ramas obsoletas"}</span><button class="btn sm" onClick={() => setCleanDlg(new Set(stale()!.filter((x) => x.merged).map((x) => x.name)))}>Limpiar…</button></div>
            </Show>
            <Show when={ghosts()?.length}>
              <div class="staleBar"><span>{ghosts()!.length} {ghosts()!.length === 1 ? "worktree en una unidad no conectada" : "worktrees en unidades no conectadas"}</span><button class="btn sm" onClick={() => setGhostDlg(true)}>Detalles…</button></div>
            </Show>
          </Show>
          <Show when={menu() === "wt"}>
            <div class="ftop"><button class="btn" onClick={openWt}>Nuevo worktree…</button></div>
            <div class="flist">
              <div class="fhead">Worktrees</div>
              <For each={trees() ?? []} fallback={<p class="muted">Sin worktrees</p>}>
                {(w, i) => (
                  <div class="li fo" classList={{ sel: w.path === cur() }} onClick={() => pick(w.path)}>
                    <Icon name="fileDirectory" />
                    <span class="grow ellip" title={w.path}>{w.branch ?? "(HEAD desacoplado)"}</span>
                    <small class="when ellip" style={{ "max-width": "140px" }}>{name(w.path)}</small>
                    <Show when={i() > 0 && w.path !== cur()}>
                      <button class="ibtn danger" title="Quitar worktree" onClick={(e) => { e.stopPropagation(); askWtRemove(w); }}><Icon name="trash" size={14} /></button>
                    </Show>
                  </div>
                )}
              </For>
            </div>
          </Show>
        </div>
      </Show>

      <Show when={ghCode()}>
        {(c) => (
          <>
            <div class="scrim dim" onClick={ghCancel} />
            <div class="dialog">
              <h2>Iniciar sesión en GitHub</h2>
              <p>Escribe este código en <a href="#" onClick={(e) => { e.preventDefault(); openUrl(c().verification_uri); }}>{c().verification_uri}</a>:</p>
              <p class="code">{c().user_code}</p>
              <p>Esperando autorización…</p>
              <div class="foot"><button class="btn" onClick={ghCancel}>Cancelar</button></div>
            </div>
          </>
        )}
      </Show>

      <Show when={confirmDiscard()}>
        <div class="scrim dim" onClick={() => { setConfirmDiscard(false); setDiscardFiles(undefined); }} />
        <div class="dialog">
          <h2>¿Descartar {toDiscard().length} {toDiscard().length === 1 ? "archivo" : "archivos"}?</h2>
          <p>Los cambios se perderán y no se pueden recuperar. Los archivos nuevos sin seguimiento se borrarán del disco.</p>
          <div class="foot">
            <button class="btn" onClick={() => { setConfirmDiscard(false); setDiscardFiles(undefined); }}>Cancelar</button>{" "}
            <button class="btn danger" onClick={doDiscard}>Descartar cambios</button>
          </div>
        </div>
      </Show>

      <Show when={blocked()}>
        <div class="scrim dim" onClick={() => setBlocked(undefined)} />
        <div class="dialog">
          <h2>Cambiar a la rama «{blocked()}»</h2>
          <p>Tienes cambios locales que se sobrescribirían al cambiar de rama.</p>
          <label class="opt" onClick={() => switchTo(blocked()!, "Stash")}>
            <b>Dejar mis cambios en «{branch()}»</b>
            <small>Se guardan en un stash y los puedes recuperar después.</small>
          </label>
          <label class="opt" onClick={() => switchTo(blocked()!, "Carry")}>
            <b>Llevar mis cambios a «{blocked()}»</b>
            <small>Se guardan en un stash, se cambia de rama y se reaplican.</small>
          </label>
          <div class="foot"><button class="btn" onClick={() => setBlocked(undefined)}>Cancelar</button></div>
        </div>
      </Show>

      <Show when={cloning()}>
        <div class="scrim dim" onClick={() => !cloneBusy() && setCloning(null)} />
        <div class="dialog clone">
          <h2>Clonar un repositorio</h2>
          <div class="ftabs">
            <span classList={{ on: cloning() === "gh" }} onClick={() => setCloning("gh")}>GitHub.com</span>
            <span classList={{ on: cloning() === "url" }} onClick={() => setCloning("url")}>URL</span>
          </div>
          <Show when={cloning() === "gh"}>
            <Show when={ghUser()} fallback={<p>Inicia sesión en GitHub para ver tus repositorios. <button class="btn" onClick={ghLogin} disabled={!!ghCode()}>Iniciar sesión</button></p>}>
              <div class="fsearch"><Icon name="search" /><input placeholder="Filtrar tus repositorios" value={cloneFilter()} onInput={(e) => setCloneFilter(e.currentTarget.value)} /></div>
              <div class="flist cl">
                <Show when={ghRepos.loading}><p class="muted">Cargando…</p></Show>
                <For each={ghShown()}>
                  {(r) => (
                    <div class="li fo" classList={{ sel: cloneUrl() === r.clone_url }} onClick={() => { setCloneUrl(r.clone_url); setCloneName(r.name); }}>
                      <Icon name={r.private ? "lock" : "repo"} /><span class="grow ellip">{r.full_name}</span>
                    </div>
                  )}
                </For>
              </div>
            </Show>
          </Show>
          <Show when={cloning() === "url"}>
            <div class="fsearch"><input placeholder="https://github.com/usuario/repo.git" value={cloneUrl()} onInput={(e) => { setCloneUrl(e.currentTarget.value); setCloneName(nameFromUrl(e.currentTarget.value)); }} /></div>
          </Show>
          <div class="fsearch"><input placeholder="Carpeta del repositorio" value={cloneName()} onInput={(e) => setCloneName(e.currentTarget.value)} /></div>
          <div class="ftop">
            <div class="fsearch"><input readOnly placeholder="Carpeta donde clonar" value={cloneDir()} /></div>
            <button class="btn" onClick={chooseDir}>Elegir…</button>
          </div>
          <div class="foot">
            <Show when={cloneBusy()}><small>{prog() ? `${prog()!.label} ${prog()!.pct}%` : "Clonando…"}</small>{" "}</Show>
            <button class="btn" disabled={cloneBusy()} onClick={() => setCloning(null)}>Cancelar</button>{" "}
            <button class="btn primary" disabled={cloneBusy() || !cloneUrl().trim() || !cloneName().trim() || !cloneDir()} onClick={doClone}>Clonar</button>
          </div>
        </div>
      </Show>
      <Show when={confirmDel()} keyed>
        {(c) => (<>
          <div class="scrim dim" style={{ "z-index": 90 }} onClick={() => setConfirmDel(undefined)} />
          <div class="dialog confirm" style={{ "z-index": 91 }} role="alertdialog" aria-label={c.title}>
            <h2>{c.title}</h2>
            <p class="prewrap">{c.body}</p>
            <div class="foot"><button class="btn" autofocus onClick={() => setConfirmDel(undefined)}>Cancelar</button><button class="btn danger" onClick={() => { setConfirmDel(undefined); c.run(); }}>{c.label}</button></div>
          </div>
        </>)}
      </Show>
      <Show when={delRemote()}>
        <div class="scrim dim" onClick={() => setDelRemote(undefined)} />
        <div class="dialog">
          <h2>¿Borrar «{delRemote()}» en origin?</h2>
          <p>Se elimina la rama del servidor para todo el equipo. Tu rama local, si existe, no se toca. Si hay un Pull Request abierto desde ella, GitHub lo cerrará.</p>
          <div class="foot"><button class="btn" onClick={() => setDelRemote(undefined)}>Cancelar</button>{" "}<button class="btn danger" onClick={doDelRemote}>Borrar en origin</button></div>
        </div>
      </Show>
      <Show when={pubDlg()}>
        <div class="scrim dim" onClick={() => !pubBusy() && setPubDlg(false)} />
        <div class="dialog">
          <h2>Publicar repositorio en GitHub</h2>
          <Show when={ghUser()} fallback={<p>Inicia sesión en GitHub para publicar. <button class="btn" onClick={ghLogin} disabled={!!ghCode()}>Iniciar sesión</button></p>}>
            <p>Se crea en tu cuenta ({ghUser()!.login}), se enlaza como <b>origin</b> y se sube la rama actual.</p>
            <label class="lbl">Nombre<div class="fsearch"><input autofocus value={pubName()} onInput={(e) => setPubName(e.currentTarget.value)} /></div></label>
            <label class="lbl">Descripción<div class="fsearch"><input placeholder="Opcional" value={pubDesc()} onInput={(e) => setPubDesc(e.currentTarget.value)} /></div></label>
            <label class="chk"><input type="checkbox" checked={pubPrivate()} onChange={(e) => setPubPrivate(e.currentTarget.checked)} /> Repositorio privado</label>
          </Show>
          <div class="foot">
            <Show when={pubBusy()}><small>{prog() ? `${prog()!.label} ${prog()!.pct}%` : "Publicando…"}</small>{" "}</Show>
            <button class="btn" disabled={pubBusy()} onClick={() => setPubDlg(false)}>Cancelar</button>{" "}
            <button class="btn primary" disabled={pubBusy() || !ghUser() || !pubName().trim()} onClick={doPublish}>Publicar</button>
          </div>
        </div>
      </Show>
      <Show when={prNew()}>
        <div class="scrim dim" onClick={() => !prBusy() && setPrNew(false)} />
        <div class="dialog">
          <h2>Nuevo Pull Request</h2>
          <p class="mono">{branch()} → {npBase() || "…"}</p>
          <label class="lbl">Rama base
            <select class="sel" value={npBase()} onChange={(e) => setNpBase(e.currentTarget.value)}>
              <For each={[...new Set([defBranch() ?? "", ...npBaseOpts()].filter(Boolean))]}>{(b) => <option value={b} selected={b === npBase()}>{b}</option>}</For>
            </select>
          </label>
          <label class="lbl">Título<div class="fsearch"><input autofocus value={npTitle()} onInput={(e) => setNpTitle(e.currentTarget.value)} /></div></label>
          <label class="lbl">Descripción<textarea class="ta" rows="5" placeholder="Qué cambia y por qué (opcional)" value={npBody()} onInput={(e) => setNpBody(e.currentTarget.value)} /></label>
          <label class="chk"><input type="checkbox" checked={npDraft()} onChange={(e) => setNpDraft(e.currentTarget.checked)} /> Crear como borrador</label>
          <Show when={!sync()?.upstream || sync()?.ahead}><p class="muted pad0">La rama se publicará en origin antes de crear el PR.</p></Show>
          <div class="foot">
            <Show when={prBusy()}><small>{prog() ? `${prog()!.label} ${prog()!.pct}%` : "Creando…"}</small>{" "}</Show>
            <button class="btn" disabled={prBusy()} onClick={() => setPrNew(false)}>Cancelar</button>{" "}
            <button class="btn primary" disabled={prBusy() || !npTitle().trim() || !npBase() || npBase() === branch()} onClick={createPr}>Crear Pull Request</button>
          </div>
        </div>
      </Show>
      <Show when={aiDlg()}>
        {(d) => {
          const prov = () => AI_PROVIDERS.find((p) => p.base === d().base);
          const sameHost = () => aiCfg()?.has_key && d().base === aiCfg()?.base_url;
          return (
            <>
              <div class="scrim dim" onClick={() => setAiDlg(undefined)} />
              <div class="dialog">
                <h2>Configuración de IA</h2>
                <p>Redacta el mensaje de commit con el diff de los archivos marcados. Sirve cualquier API compatible con OpenAI.</p>
                <label class="lbl">Proveedor
                  <select class="sel" value={prov()?.name ?? ""} onChange={(e) => { const p = AI_PROVIDERS.find((x) => x.name === e.currentTarget.value); setAiDlg({ ...d(), base: p?.base ?? "", model: p?.model ?? d().model }); }}>
                    <For each={AI_PROVIDERS}>{(p) => <option value={p.name}>{p.name}</option>}</For>
                    <option value="">Personalizado</option>
                  </select>
                </label>
                <label class="lbl">URL de la API<div class="fsearch"><input placeholder="https://…/v1" value={d().base} onInput={(e) => setAiDlg({ ...d(), base: e.currentTarget.value })} /></div></label>
                <label class="lbl">Modelo<div class="fsearch"><input placeholder="Nombre del modelo" value={d().model} onInput={(e) => setAiDlg({ ...d(), model: e.currentTarget.value })} /></div></label>
                <label class="lbl">Clave de API
                  <div class="fsearch"><input type="password" autocomplete="off" value={d().key} onInput={(e) => setAiDlg({ ...d(), key: e.currentTarget.value })}
                    placeholder={sameHost() ? "Guardada en el llavero (vacío = conservarla)" : prov()?.name === "Ollama" ? "No hace falta en local" : "Pega tu clave"} /></div>
                </label>
                <Show when={prov()?.keys}><p><a href="#" onClick={(e) => { e.preventDefault(); openUrl(prov()!.keys); }}>Conseguir una clave de {prov()!.name}</a></p></Show>
                <div class="foot">
                  <Show when={aiOn()}><button class="btn" onClick={() => saveAi(true)}>Apagar IA</button>{" "}</Show>
                  <button class="btn" onClick={() => setAiDlg(undefined)}>Cancelar</button>{" "}
                  <button class="btn primary" disabled={!d().base.trim() || !d().model.trim()} onClick={() => saveAi()}>Guardar</button>
                </div>
              </div>
            </>
          );
        }}
      </Show>
      <Show when={editRepo()}>
        {(e) => (
          <>
            <div class="scrim dim" onClick={() => setEditRepo(undefined)} />
            <div class="dialog">
              <h2>Alias del repositorio</h2>
              <p class="mono ellip" title={e().path}>{e().path}</p>
              <label class="lbl">Alias<div class="fsearch"><input autofocus placeholder={label(e().path)} value={e().alias} onInput={(ev) => setEditRepo({ ...e(), alias: ev.currentTarget.value })} /></div></label>
              <label class="lbl">Grupo
                <div class="fsearch"><input list="groups-dl" placeholder="Ej. Ascend (vacío = agrupar por dueño del remoto)" value={e().group} onInput={(ev) => setEditRepo({ ...e(), group: ev.currentTarget.value })} /></div>
              </label>
              <datalist id="groups-dl"><For each={knownGroups()}>{(g) => <option value={g} />}</For></datalist>
              <Show when={name(dirname(e().path)) && name(dirname(e().path)) !== e().group}>
                <p><button class="btn sm" onClick={() => setEditRepo({ ...e(), group: name(dirname(e().path)) })}>Usar la carpeta «{name(dirname(e().path))}»</button></p>
              </Show>
              <div class="foot">
                <button class="btn" onClick={() => setEditRepo(undefined)}>Cancelar</button>{" "}
                <button class="btn primary" onClick={saveMeta}>Guardar</button>
              </div>
            </div>
          </>
        )}
      </Show>
      <Show when={ctx()} keyed>
        {(c) => (
          <>
            <div class="scrim" style={{ "z-index": 60 }} onClick={() => setCtx(undefined)} onContextMenu={(e) => { e.preventDefault(); setCtx(undefined); }} />
            <div class="ctxmenu" style={{ left: `${c.x}px`, top: `${c.y}px` }}>
              <button onClick={() => ctxDo(() => setInfoPath(c.path))}>Información…</button>
              <button onClick={() => ctxDo(() => { setNewGroup(""); setMoving(c.path); })}>Mover a un grupo…</button>
              <button onClick={() => ctxDo(() => { const r = repos()?.find((x) => x.path === c.path); if (r) setEditRepo({ path: r.path, alias: r.alias, group: r.group }); })}>Cambiar alias…</button>
              <hr />
              <button onClick={() => ctxDo(() => commands.openIn("Folder", c.path))}>Mostrar en la carpeta</button>
              <button onClick={() => ctxDo(() => commands.openIn("Terminal", c.path))}>Abrir en la terminal</button>
              <button onClick={() => ctxDo(() => commands.openIn("Editor", c.path))}>Abrir en el editor</button>
              <hr />
              <button class="danger" onClick={() => ctxDo(() => remove(c.path))}>Quitar de la lista</button>
            </div>
          </>
        )}
      </Show>
      <Show when={fctx()} keyed>
        {(f) => (
          <>
            <div class="scrim" style={{ "z-index": 60 }} onClick={() => setFctx(undefined)} onContextMenu={(e) => { e.preventDefault(); setFctx(undefined); }} />
            <div class="ctxmenu wide" style={{ left: `${f.x}px`, top: `${f.y}px` }}>
              <button class="danger" onClick={() => fctxDo(() => { setDiscardFiles([f.path]); setConfirmDiscard(true); })}>Descartar cambios…</button>
              <Show when={f.kind === "Added"}>
                <button onClick={() => fctxDo(async () => { await commands.ignoreFile(cur()!, f.path); refetch(); })}>Ignorar archivo (añadir a .gitignore)</button>
              </Show>
              <hr />
              <Show when={f.kind !== "Added"}>
                <button onClick={() => fctxDo(() => { setFFile(f.path); setTab("history"); })}>Ver historial del archivo</button>
              </Show>
              <button onClick={() => fctxDo(() => navigator.clipboard.writeText(absPath(f.path)))}>Copiar ruta del archivo</button>
              <button onClick={() => fctxDo(() => navigator.clipboard.writeText(f.path))}>Copiar ruta relativa</button>
              <hr />
              <button onClick={() => fctxDo(() => commands.fileAction("Reveal", cur()!, f.path))}>{isMac ? "Mostrar en Finder" : "Mostrar en el explorador"}</button>
              <button onClick={() => fctxDo(() => commands.fileAction("Editor", cur()!, f.path))}>Abrir en el editor</button>
              <button onClick={() => fctxDo(() => commands.fileAction("Default", cur()!, f.path))}>Abrir con el programa predeterminado</button>
            </div>
          </>
        )}
      </Show>
      <Show when={infoPath()} keyed>
        {(p) => (
          <>
            <div class="scrim dim" onClick={() => setInfoPath(undefined)} />
            <div class="dialog info">
              <h2>{label(p)}</h2>
              <Show when={info()} fallback={<p class="muted">{info.error ? String(info.error) : "Cargando…"}</p>}>
                {(i) => (
                  <dl class="kv">
                    <dt>Nombre del repo</dt><dd>{i().repo ?? <>{name(p)} <small class="when">(sin remoto: nombre de la carpeta)</small></>}</dd>
                    <Show when={repos()?.find((r) => r.path === p)?.alias}><dt>Alias</dt><dd>{repos()?.find((r) => r.path === p)?.alias}</dd></Show>
                    <dt>Dueño</dt><dd>{i().owner ?? "—"}</dd>
                    <dt>Carpeta</dt><dd class="mono" title={p}>{p}</dd>
                    <dt>Remoto</dt><dd class="mono">{i().remote_url ?? "sin remoto (solo local)"}</dd>
                    <Show when={priv()[p] !== undefined}><dt>Visibilidad</dt><dd>{priv()[p] ? "Privado" : "Público"}</dd></Show>
                    <dt>Grupo</dt><dd>{repos()?.find((r) => r.path === p)?.group || "— (por dueño del remoto)"}</dd>
                    <dt>Rama actual</dt><dd>{i().branch}</dd>
                    <dt>Ramas locales</dt><dd>{i().branches}</dd>
                    <dt>Commits</dt><dd>{i().commits}</dd>
                    <dt>Último commit</dt><dd>{i().last_commit || "—"}</dd>
                    <Show when={i().last_commit_time}>
                      <dt>Fecha y autor</dt><dd>{new Date(i().last_commit_time! * 1000).toLocaleString("es", { dateStyle: "long", timeStyle: "short" })} <small class="when">· {when(i().last_commit_time!)}</small><Show when={i().last_author}> · {i().last_author}</Show></dd>
                    </Show>
                    <dt>Cambios locales</dt><dd>{i().changes}</dd>
                    <dt>Stashes</dt><dd>{i().stashes}</dd>
                  </dl>
                )}
              </Show>
              <div class="foot"><button class="btn primary" onClick={() => setInfoPath(undefined)}>Cerrar</button></div>
            </div>
          </>
        )}
      </Show>
      <Show when={creating()}>
        <div class="scrim dim" onClick={() => setCreating(false)} />
        <div class="dialog">
          <h2>Nueva rama</h2>
          <div class="fsearch"><Icon name="gitBranch" /><input autofocus placeholder="Nombre de la rama" value={newBranch()} onInput={(e) => setNewBranch(e.currentTarget.value)} onKeyDown={(e) => e.key === "Enter" && newBranch().trim() && createBranch()} /></div>
          <p>Se crea a partir de «{branch()}». Tus cambios locales se mantienen.</p>
          <Show when={sync()?.has_origin}>
            <label class="chk"><input type="checkbox" checked={publishNew()} onChange={(e) => setPublishNew(e.currentTarget.checked)} /> Publicar también en origin</label>
          </Show>
          <div class="foot">
            <button class="btn" onClick={() => setCreating(false)}>Cancelar</button>{" "}
            <button class="btn primary" disabled={!newBranch().trim()} onClick={createBranch}>Crear rama</button>
          </div>
        </div>
      </Show>
      <Show when={wtDlg()}>
        <div class="scrim dim" onClick={() => setWtDlg(false)} />
        <div class="dialog">
          <h2>Nuevo worktree</h2>
          <p>Otra carpeta con su propia rama del mismo repositorio, para trabajar en paralelo sin cambiar de rama.</p>
          <div class="ftabs"><span classList={{ on: wtNew() }} onClick={() => setWtNew(true)}>Rama nueva</span><span classList={{ on: !wtNew() }} onClick={() => setWtNew(false)}>Rama existente</span></div>
          <Show when={wtNew()} fallback={
            <select class="sel" value={wtBranch()} onChange={(e) => setWtBranch(e.currentTarget.value)}>
              <option value="">Elige una rama…</option>
              <For each={(wtBranches() ?? []).filter((b) => b.name !== branch())}>{(b) => <option value={b.name}>{b.name}</option>}</For>
            </select>
          }>
            <div class="fsearch"><Icon name="gitBranch" /><input placeholder="Nombre de la rama nueva" value={wtBranch()} onInput={(e) => setWtBranch(e.currentTarget.value)} /></div>
          </Show>
          <div class="ftop">
            <div class="fsearch"><input readOnly placeholder="Carpeta donde crearlo" value={wtParent()} /></div>
            <button class="btn" onClick={async () => { const d = await open({ directory: true, title: "Carpeta del worktree" }); if (typeof d === "string") setWtParent(d); }}>Elegir…</button>
          </div>
          <p class="muted pad0">Se creará en: <span class="mono">{wtDest() || "—"}</span></p>
          <div class="foot">
            <button class="btn" onClick={() => setWtDlg(false)}>Cancelar</button>{" "}
            <button class="btn primary" disabled={!wtBranch().trim() || !wtParent()} onClick={doWorktree}>Crear worktree</button>
          </div>
        </div>
      </Show>
      <Show when={prOffer()}><div class="banner ok"><span class="banner-msg">«{prOffer()}» subida. <button class="btn sm primary" onClick={() => { setPrOffer(undefined); setTab("prs"); openNewPr(); }}>Crear Pull Request…</button></span><button class="x" onClick={() => setPrOffer(undefined)}><Icon name="x" size={14} /></button></div></Show>
      <Show when={note()}><div class="banner ok"><span class="banner-msg">{note()}</span><button class="x" onClick={() => setNote("")}><Icon name="x" size={14} /></button></div></Show>
      <Show when={mergeBr()} keyed>
        {(b) => (<>
          <div class="scrim dim" onClick={() => setMergeBr(undefined)} />
          <div class="dialog">
            <h2>Fusionar «{b}» en «{branch()}»</h2>
            <p>Se traen a «{branch()}» los commits de «{b}» que aún no tiene. Si hay conflictos podrás resolverlos o abortar y todo vuelve a como estaba.</p>
            <div class="foot"><button class="btn" onClick={() => setMergeBr(undefined)}>Cancelar</button><button class="btn primary" onClick={() => doMerge(b)}>Fusionar</button></div>
          </div>
        </>)}
      </Show>
      <Show when={opAsk()} keyed>
        {(o) => (<>
          <div class="scrim dim" onClick={() => setOpAsk(undefined)} />
          <div class="dialog">
            <h2>{o.kind === "pick" ? `Cherry-pick del commit ${o.id}` : `Revertir el commit ${o.id}`}</h2>
            <p>{o.kind === "pick"
              ? `Copia ese commit${o.from ? ` de «${o.from}»` : ""} a «${branch()}» como un commit nuevo. El original no se toca y «${o.from ?? "la otra rama"}» sigue igual. Sirve para traer un arreglo concreto sin fusionar toda su rama.`
              : `Crea un commit nuevo en «${branch()}» que deshace los cambios de ese commit. No borra historial, así que es seguro aunque ya esté subido.`}</p>
            <p class="hint">Si hay conflictos podrás resolverlos o abortar y todo vuelve a como estaba.</p>
            <label class="opt-check"><input type="checkbox" onChange={(e) => { try { localStorage.setItem(skipKey, e.currentTarget.checked ? "1" : "0"); } catch { /* sin almacenamiento: se vuelve a preguntar */ } }} /> No volver a preguntar</label>
            <div class="foot"><button class="btn" onClick={() => setOpAsk(undefined)}>Cancelar</button><button class="btn primary" onClick={() => { setOpAsk(undefined); o.kind === "pick" ? doPick(o.id) : doRevert(o.id); }}>{o.kind === "pick" ? "Aplicar" : "Revertir"}</button></div>
          </div>
        </>)}
      </Show>
      <Show when={helpOpen()}>
        <div class="scrim dim" onClick={() => setHelpOpen(false)} />
        <div class="dialog help" role="dialog" aria-label="Ayuda">
          <div class="helphead"><h2>Ayuda</h2><button class="x" title="Cerrar" onClick={() => setHelpOpen(false)}><Icon name="x" size={16} /></button></div>
          <div class="helpcols">
            <nav class="helpnav" aria-label="Temas">
              <For each={[...new Set(TOPICS.map((t) => t.group))]}>
                {(g) => (<>
                  <small>{g}</small>
                  <For each={TOPICS.filter((t) => t.group === g)}>
                    {(t) => <button classList={{ on: helpSel() === t.id }} onClick={() => setHelpSel(t.id)}><Icon name={t.icon} size={15} /><span class="ellip">{t.title}</span></button>}
                  </For>
                </>)}
              </For>
            </nav>
            <Show when={TOPICS.find((t) => t.id === helpSel())} keyed>
              {(t) => (
                <article class="helpdetail">
                  <div class="hd"><span class="big"><Icon name={t.icon} size={26} /></span><div><small>{t.group}</small><h3>{t.title}</h3></div></div>
                  <p class="what">{t.what}</p>
                  <Show when={t.when}><h4>Cuándo usarlo</h4><p>{t.when}</p></Show>
                  <Show when={t.how}><h4>Cómo hacerlo en Grove</h4><p>{t.how}</p></Show>
                  <Show when={t.note}><div class="helpnote"><Icon name="warn" size={15} /><span>{t.note}</span></div></Show>
                </article>
              )}
            </Show>
          </div>
        </div>
      </Show>
      <Show when={ghostDlg()}>
        <div class="scrim dim" onClick={() => setGhostDlg(false)} />
        <div class="dialog">
          <h2>Worktrees en unidades no conectadas</h2>
          <p>Estas carpetas no se encuentran porque su disco o recurso de red no está disponible ahora. Grove no las olvida solo: si reconectas la unidad, vuelven a funcionar.</p>
          <div class="conflicts"><For each={ghosts() ?? []}>{(w) => <div class="crow"><span class="mono ellip" title={w.path}>{w.path}</span><small>{w.branch ?? "(sin rama)"}</small></div>}</For></div>
          <p class="hint">«Olvidar» solo quita su registro en git; no borra ramas ni commits. Si luego reconectas el disco, <span class="mono">git worktree repair</span> lo recupera.</p>
          <div class="foot"><button class="btn" onClick={() => setGhostDlg(false)}>Mantener</button><button class="btn danger" onClick={forgetGhosts}>Olvidar registros</button></div>
        </div>
      </Show>
      <Show when={cleanDlg()} keyed>
        {(sel) => (<>
          <div class="scrim dim" onClick={() => !cleanBusy() && setCleanDlg(undefined)} />
          <div class="dialog">
            <h2>Limpiar ramas obsoletas</h2>
            <p>Estas ramas locales ya no existen en GitHub. Las que están fusionadas en otra rama vienen marcadas: borrarlas no pierde ningún commit.</p>
            <div class="conflicts">
              <For each={stale() ?? []}>
                {(b) => (
                  <label class="crow stale">
                    <input type="checkbox" checked={sel.has(b.name)} onChange={(e) => { const n = new Set(sel); e.currentTarget.checked ? n.add(b.name) : n.delete(b.name); setCleanDlg(n); }} />
                    <span class="mono ellip" title={b.name}>{b.name}</span>
                    <small class={b.merged ? "okmsg" : "warnmsg"}>{b.merged ? "fusionada" : "commits sin fusionar"}</small>
                  </label>
                )}
              </For>
            </div>
            <Show when={(stale() ?? []).some((b) => !b.merged && sel.has(b.name))}><div class="helpnote"><Icon name="warn" size={15} /><span>Has marcado ramas con commits que no están en ninguna otra rama: al borrarlas se pierden.</span></div></Show>
            <div class="foot"><button class="btn" disabled={cleanBusy()} onClick={() => setCleanDlg(undefined)}>Cancelar</button><button class="btn primary" disabled={!sel.size || cleanBusy()} onClick={cleanBranches}>{cleanBusy() ? "Borrando…" : `Borrar ${sel.size}`}</button></div>
          </div>
        </>)}
      </Show>
      <Show when={palOpen()}>
        <div class="scrim dim" onClick={() => setPalOpen(false)} />
        <div class="palette" role="dialog" aria-label="Paleta de comandos">
          <div class="fsearch"><Icon name="search" /><input autofocus placeholder="Buscar repositorio, rama o acción…" value={palQ()} onInput={(e) => setPalQ(e.currentTarget.value)} onKeyDown={palKey} /></div>
          <div class="pallist" ref={(el) => createEffect(() => { palIdx(); queueMicrotask(() => el.querySelector(".on")?.scrollIntoView({ block: "nearest" })); })}>
            <For each={palItems()} fallback={<p class="muted">Nada coincide con «{palQ()}»</p>}>
              {(it, i) => (<>
                <Show when={i() === 0 || palItems()[i() - 1].group !== it.group}><small class="palgroup">{it.group}</small></Show>
                <button class="palitem" classList={{ on: i() === palIdx() }} onMouseMove={() => setPalIdx(i())} onClick={() => palRun(it)}>
                  <Icon name={it.icon} size={15} /><span class="grow ellip">{it.label}</span><small class="ellip">{it.hint}</small>
                </button>
              </>)}
            </For>
          </div>
          <div class="palfoot"><span>↑↓ navegar</span><span>⏎ elegir</span><span>Esc cerrar</span></div>
        </div>
      </Show>
      <Show when={cmpOpen()}>
        <div class="scrim dim" onClick={() => setCmpOpen(false)} />
        <div class="dialog compare" role="dialog" aria-label="Comparar ramas">
          <div class="helphead"><h2>Comparar ramas</h2><button class="x" title="Cerrar" onClick={() => setCmpOpen(false)}><Icon name="x" size={16} /></button></div>
          <div class="cmpsel">
            <label>Base<select value={cmpBase()} onChange={(e) => setCmpBase(e.currentTarget.value)}><For each={cmpBranches() ?? []}>{(b) => <option value={b.name} selected={b.name === cmpBase()}>{b.name}</option>}</For></select></label>
            <Icon name="gitMerge" size={16} />
            <label>Comparar con<select value={cmpOther()} onChange={(e) => setCmpOther(e.currentTarget.value)}><option value="">Elige una rama…</option><For each={(cmpBranches() ?? []).filter((b) => b.name !== cmpBase())}>{(b) => <option value={b.name} selected={b.name === cmpOther()}>{b.name}</option>}</For></select></label>
          </div>
          <Show when={cmp()} keyed fallback={<p class="muted cmpempty">{cmp.loading ? "Comparando…" : "Elige la rama con la que comparar. Verás qué cambia respecto a la base, como en un Pull Request."}</p>}>
            {(c) => (<>
              <div class="cmpsum">
                <span><b>«{cmpOther()}»</b> tiene <b>{c.ahead}</b> {c.ahead === 1 ? "commit" : "commits"} que «{cmpBase()}» no</span>
                <span>«{cmpBase()}» tiene <b>{c.behind}</b> {c.behind === 1 ? "commit" : "commits"} que «{cmpOther()}» no</span>
                <Show when={cmpBase() === branch() && c.ahead > 0}><button class="btn sm" onClick={() => { setCmpOpen(false); setTab("history"); setFRev(cmpOther()); setFOnlyNew(true); }}>Ver esos commits</button></Show>
              </div>
              <div class="cmpcols">
                <div class="cmpfiles">
                  <For each={c.files} fallback={<p class="muted">Sin diferencias de archivos.</p>}>
                    {(f) => (
                      <button classList={{ on: cmpFile() === f.path }} onClick={() => setCmpFile(f.path)} title={f.path}>
                        <Icon name={f.kind === "Added" ? "diffAdded" : f.kind === "Removed" ? "diffRemoved" : "diffModified"} size={14} />
                        <span class="ellip">{f.path}</span>
                        <small><span class="plus">+{f.additions}</span> <span class="minus">−{f.deletions}</span></small>
                      </button>
                    )}
                  </For>
                </div>
                <div class="cmpdiff"><Show when={cmpDiff()} fallback={<p class="muted">{cmpDiff.loading ? "Cargando…" : "Elige un archivo."}</p>}><DiffView text={cmpDiff()!} /></Show></div>
              </div>
            </>)}
          </Show>
        </div>
      </Show>
      <Show when={reflogOpen()}>
        <div class="scrim dim" onClick={() => setReflogOpen(false)} />
        <div class="dialog reflog">
          <h2>Recuperar</h2>
          <p>Todos los sitios donde ha estado HEAD en este repositorio. Si un reset, un borrado o una operación abortada te hizo «perder» commits, siguen aquí: crea una rama en ese punto para recuperarlos.</p>
          <div class="conflicts rl">
            <Show when={!reflog.loading} fallback={<p class="muted">Cargando…</p>}>
              <For each={reflog() ?? []} fallback={<p class="muted">Sin movimientos registrados</p>}>
                {(e) => (
                  <div class="crow rl">
                    <span class="mono">{e.id}</span>
                    <span class="grow"><b class="ellip">{e.summary}</b><small class="ellip">{e.action} · {ago(e.time)}</small></span>
                    <button class="btn sm" onClick={() => setBranchAt({ rev: e.id, name: `recuperado-${e.id}`, sw: false })}>Crear rama…</button>
                  </div>
                )}
              </For>
            </Show>
          </div>
          <div class="foot"><button class="btn primary" onClick={() => setReflogOpen(false)}>Cerrar</button></div>
        </div>
      </Show>
      <Show when={branchAt()} keyed>
        {(d) => (<>
          <div class="scrim dim" style={{ "z-index": 70 }} onClick={() => !opBusy() && setBranchAt(undefined)} />
          <div class="dialog" style={{ "z-index": 71 }}>
            <h2>Crear rama en {d.rev}</h2>
            <label class="lbl">Nombre de la rama<div class="fsearch"><input autofocus placeholder="mi-rama" value={d.name} onInput={(e) => setBranchAt({ ...d, name: e.currentTarget.value })} onKeyDown={(e) => e.key === "Enter" && doBranchAt()} /></div></label>
            <label class="opt-check"><input type="checkbox" checked={d.sw} onChange={(e) => setBranchAt({ ...d, sw: e.currentTarget.checked })} /> Cambiar a la rama nueva</label>
            <div class="foot"><button class="btn" disabled={opBusy()} onClick={() => setBranchAt(undefined)}>Cancelar</button><button class="btn primary" disabled={!d.name.trim() || opBusy()} onClick={doBranchAt}>{opBusy() ? "Creando…" : "Crear rama"}</button></div>
          </div>
        </>)}
      </Show>
      <Show when={resetDlg()} keyed>
        {(d) => (<>
          <div class="scrim dim" onClick={() => !opBusy() && setResetDlg(undefined)} />
          <div class="dialog">
            <h2>Volver a {d.id}</h2>
            <p class="ellip" title={d.summary}>«{d.summary}»</p>
            <p>{d.n === null ? "Calculando…" : d.n === 0 ? "Ya estás en ese commit." : `«${branch()}» perderá ${d.n} ${d.n === 1 ? "commit" : "commits"} (los posteriores a ese).`}</p>
            <label class="opt" classList={{ chosen: d.mode === "mixed" }} onClick={() => setResetDlg({ ...d, mode: "mixed" })}><b>Conservar los cambios</b><small>Los cambios de esos commits vuelven a Cambios, sin commitear, para que puedas rehacerlos.</small></label>
            <label class="opt danger" classList={{ chosen: d.mode === "hard" }} onClick={() => setResetDlg({ ...d, mode: "hard" })}><b>Descartar todo</b><small>Se pierden los cambios de esos commits y también los que tengas sin commitear{changes()?.length ? ` (${changes()!.length} ${changes()!.length === 1 ? "archivo" : "archivos"})` : ""}.</small></label>
            <p class="hint">Los commits quitados no se borran del todo: puedes recuperarlos en «Recuperar».{sync()?.upstream && (d.n ?? 0) > 0 ? " Si ya los habías subido, para subir la rama tendrás que forzar el push." : ""}</p>
            <div class="foot"><button class="btn" disabled={opBusy()} onClick={() => setResetDlg(undefined)}>Cancelar</button><button class={d.mode === "hard" ? "btn danger" : "btn primary"} disabled={!d.n || opBusy()} onClick={doReset}>{opBusy() ? "Aplicando…" : d.mode === "hard" ? "Descartar y volver" : "Volver a este commit"}</button></div>
          </div>
        </>)}
      </Show>
      <Show when={tagDlg()} keyed>
        {(d) => (<>
          <div class="scrim dim" onClick={() => !tagBusy() && setTagDlg(undefined)} />
          <div class="dialog">
            <h2>Crear tag en {d.id}</h2>
            <label class="lbl">Nombre<div class="fsearch"><input autofocus placeholder="v1.0.0" value={d.name} onInput={(e) => setTagDlg({ ...d, name: e.currentTarget.value })} onKeyDown={(e) => e.key === "Enter" && createTag()} /></div></label>
            <label class="lbl">Mensaje (opcional: crea un tag anotado)<div class="fsearch"><input value={d.msg} onInput={(e) => setTagDlg({ ...d, msg: e.currentTarget.value })} onKeyDown={(e) => e.key === "Enter" && createTag()} /></div></label>
            <label class="opt-check"><input type="checkbox" checked={d.push} onChange={(e) => setTagDlg({ ...d, push: e.currentTarget.checked })} /> Subir a origin</label>
            <div class="foot"><button class="btn" disabled={tagBusy()} onClick={() => setTagDlg(undefined)}>Cancelar</button><button class="btn primary" disabled={!d.name.trim() || tagBusy()} onClick={createTag}>{tagBusy() ? "Creando…" : "Crear tag"}</button></div>
          </div>
        </>)}
      </Show>
      <Show when={cctx()} keyed>
        {(c) => (<>
          <div class="scrim" style={{ "z-index": 60 }} onClick={() => setCctx(undefined)} onContextMenu={(e) => { e.preventDefault(); setCctx(undefined); }} />
          <div class="ctxmenu wide" style={{ left: `${c.x}px`, top: `${c.y}px` }}>
            <Show when={fRev()} fallback={<>
              <button disabled title="Ese commit ya está en la rama actual. Elige otra rama arriba, en «Historial de…»"><Icon name="cherry" size={14} /> Cherry-pick (elige otra rama arriba)</button>
              <button onClick={() => askOp("revert", c.id)}><Icon name="undo" size={14} /> Revertir este commit</button>
            </>}>
              <button onClick={() => askOp("pick", c.id)}><Icon name="cherry" size={14} /> Cherry-pick a «{branch()}»</button>
            </Show>
            <button onClick={() => { setCctx(undefined); setTagDlg({ id: c.id, name: "", msg: "", push: true }); }}><Icon name="tag" size={14} /> Crear tag aquí…</button>
            <button onClick={() => { setCctx(undefined); setBranchAt({ rev: c.id, name: "", sw: true }); }}><Icon name="gitBranch" size={14} /> Crear rama aquí…</button>
            <Show when={!fRev()}><button class="danger" onClick={() => openReset(c)}><Icon name="undo" size={14} /> Volver a este commit…</button></Show>
            <For each={tagsOf(c.id)}>{(t) => <button class="danger" onClick={() => { setCctx(undefined); setConfirmDel({ title: `¿Borrar el tag «${t.name}»?`, body: "Solo se borra en tu equipo; si lo subiste a origin, allí sigue. Los commits no se tocan.", label: "Borrar tag", run: async () => { try { await commands.deleteTag(cur()!, t.name, false); refTags(); } catch (e) { setErr(String(e)); } } }); }}><Icon name="trash" size={14} /> Borrar tag «{t.name}» (local)</button>}</For>
            <button onClick={() => { setCctx(undefined); navigator.clipboard.writeText(c.id); }}><Icon name="copy" size={14} /> Copiar hash</button>
          </div>
        </>)}
      </Show>
      <Show when={sync()?.merging && !resolveOpen()}>
        <div class="banner warn">
          <span class="banner-msg"><b>{OPS[sync()!.op] ?? "Operación"} en curso</b>{sync()!.conflicts.length ? `: ${sync()!.conflicts.length} ${sync()!.conflicts.length === 1 ? "archivo" : "archivos"} por resolver` : ": conflictos resueltos, falta concluir"}</span>
          <span class="sact"><button class="btn primary sm" onClick={() => setResolveOpen(true)}>Resolver…</button><button class="btn sm danger" onClick={abortMerge}>Abortar</button></span>
        </div>
      </Show>
      <Show when={sync()?.merging && resolveOpen()}>
        <div class="scrim dim" onClick={() => setResolveOpen(false)} />
        <div class="dialog resolve" classList={{ wide: !!previewFile() }}>
          <h2>{OPS[sync()!.op] ?? "Operación"}: {sync()!.conflicts.length ? "hay conflictos" : "todo resuelto"}</h2>
          <p>{OP_WHY[sync()!.op]}</p>
          <Show when={sync()!.conflicts.length} fallback={<p class="okmsg">No quedan conflictos. Pulsa <b>Concluir</b> para terminar.</p>}>
            <p class="hint">Para cada archivo elige qué versión conservar: <b>Mía</b> es la de «{branch()}», <b>Suya</b> es la que llega. Si lo editaste a mano, usa <b>Ya resuelto</b>.</p>
            <div class="conflicts">
              <For each={sync()!.conflicts}>
                {(f) => (
                  <>
                    <div class="crow">
                      <span class="mono ellip" title={f}>{f}</span>
                      <button class="btn sm" classList={{ on: previewFile() === f }} title="Ver las dos versiones antes de elegir" onClick={() => setPreviewFile(previewFile() === f ? undefined : f)}>Ver</button>
                      <button class="btn sm" title="Quedarte con tu versión" onClick={() => resolve(f, "ours")}>Mía</button>
                      <button class="btn sm" title="Quedarte con la versión entrante" onClick={() => resolve(f, "theirs")}>Suya</button>
                      <button class="btn sm" title="Ya lo editaste a mano: márcalo como resuelto" onClick={() => resolve(f, "manual")}>Ya resuelto</button>
                    </div>
                    <Show when={previewFile() === f}>
                      <div class="sides">
                        <Show when={!sides.loading} fallback={<p class="muted">Cargando…</p>}>
                          <For each={[{ t: `Mía (${branch()})`, c: sides()?.[0], o: sides()?.[1] }, { t: "Suya (la que llega)", c: sides()?.[1], o: sides()?.[0] }]}>
                            {(sd) => {
                              const other = new Set((sd.o ?? "").split("\n"));
                              return (
                                <div class="side">
                                  <b>{sd.t}</b>
                                  <Show when={sd.c != null} fallback={<p class="muted">Sin contenido: el archivo se borró en este lado, es binario o es demasiado grande.</p>}>
                                    <pre><For each={sd.c!.split("\n")}>{(ln) => <div classList={{ only: !other.has(ln) }}>{ln || " "}</div>}</For></pre>
                                  </Show>
                                </div>
                              );
                            }}
                          </For>
                        </Show>
                      </div>
                      <p class="hint sidehint">Las líneas resaltadas existen solo en ese lado. Si necesitas combinar ambas, edita el archivo en tu editor y pulsa «Ya resuelto».</p>
                    </Show>
                  </>
                )}
              </For>
            </div>
          </Show>
          <div class="foot split">
            <span class="sact">
              <button class="btn" onClick={() => launch("Editor")}>Abrir en el editor</button>
              <button class="btn danger" title="Vuelve todo a como estaba antes de empezar" onClick={() => { setResolveOpen(false); abortMerge(); }}>Abortar</button>
            </span>
            <span class="sact">
              <button class="btn" onClick={() => setResolveOpen(false)}>Resolver luego</button>
              <button class="btn primary" disabled={!!sync()!.conflicts.length} onClick={mergeContinue}>Concluir</button>
            </span>
          </div>
        </div>
      </Show>
      <Show when={rejected()}>
        <div class="scrim dim" onClick={() => setRejected(false)} />
        <div class="dialog">
          <h2>El remoto tiene commits que no tienes</h2>
          <p>El push fue rechazado. Lo normal es traer los cambios (pull) y volver a subir.</p>
          <label class="opt" onClick={pullThenClose}><b>Hacer pull ahora</b><small>Fusiona los cambios remotos con los tuyos</small></label>
          <label class="opt" onClick={() => doSync(true)}><b>Forzar push</b><small>Usa --force-with-lease: sobrescribe el remoto solo si nadie ha subido nada desde tu último fetch. Puede borrar commits ajenos.</small></label>
          <div class="foot"><button class="btn" onClick={() => setRejected(false)}>Cancelar</button></div>
        </div>
      </Show>
      <Show when={err()}><div class="banner"><span class="banner-msg">
        <Show when={AUTH_ERR.test(err())} fallback={err()}>
          {ghUser() ? "GitHub rechazó las credenciales para este repositorio: tu cuenta puede no tener acceso o la sesión caducó. Cierra sesión y vuelve a iniciarla." : "Este repositorio necesita iniciar sesión en GitHub para sincronizar (fetch, pull y push)."}
          <Show when={!ghUser()}><button class="btn" onClick={() => { setErr(""); ghLogin(); }} disabled={!!ghCode()}>Iniciar sesión</button></Show>
        </Show></span>
        <button class="x" onClick={() => setErr("")}><Icon name="x" size={14} /></button></div></Show>

      <Show
        when={cur()}
        fallback={
          <div class="empty">
            <img src="/grove-logo.png" class="blank" alt="Grove" />
            <h1>Aún no hay repositorios</h1>
            <p>Añade un repositorio local para empezar.</p>
            <button class="btn primary" onClick={add}>Añadir repositorio local…</button>
          </div>
        }
      >
        <div class="body">
          <aside class="side">
            <nav class="tabs">
              <button classList={{ on: tab() === "changes" }} onClick={() => setTab("changes")}>
                {stashMode() ? "Stashes" : "Cambios"} <Show when={stashMode() ? stashes()?.length : changes()?.length}><span class="count">{stashMode() ? stashes()!.length : changes()!.length}</span></Show>
              </button>
              <button classList={{ on: tab() === "history" }} onClick={() => setTab("history")}>Historial</button>
              <button classList={{ on: tab() === "prs" }} onClick={() => setTab("prs")}>Pull Requests <Show when={tab() === "prs" && prs()?.length}><span class="count">{prs()!.length}</span></Show></button>
            </nav>

            <Show when={tab() === "prs"}>
              <Show when={ghUser()} fallback={<div class="empty-side"><p>Inicia sesión en GitHub para ver y crear Pull Requests.</p><button class="btn primary" onClick={ghLogin} disabled={!!ghCode()}>Iniciar sesión</button></div>}>
                <div class="hfilter">
                  <select value={prState()} onChange={(e) => setPrState(e.currentTarget.value as "open" | "closed" | "all")} title="Estado">
                    <option value="open">Abiertos</option><option value="closed">Cerrados</option><option value="all">Todos</option>
                  </select>
                  <button class="btn primary sm" disabled={!branch() || !sync()?.has_origin} title={sync()?.has_origin ? "Crea un PR con la rama actual" : "El repositorio no tiene origin"} onClick={openNewPr}>Nuevo PR</button>
                </div>
                <div class="list">
                  <Show when={prs.error}><p class="err">{String(prs.error)}</p></Show>
                  <Show when={prs.loading && !prs()}><p class="muted">Cargando…</p></Show>
                  <Show when={prs() && !prs()!.length}><p class="muted">No hay Pull Requests {prState() === "open" ? "abiertos" : ""}</p></Show>
                  <For each={prs() ?? []}>
                    {(pr) => (
                      <div class="li commit prrow" classList={{ sel: prSel() === pr.number }} onClick={() => setPrSel(pr.number)}>
                        <b class="ellip" title={pr.title}>{pr.title}</b>
                        <span class="meta">
                          <Avatar name={pr.author} src={pr.avatar_url} size={16} />
                          <small class="ellip">#{pr.number} · {pr.author} · {isoAgo(pr.updated_at)}</small>
                          <span class={`pill ${pr.draft && pr.state === "open" ? "draft" : pr.state}`}>{prBadge(pr.state, pr.draft)}</span>
                        </span>
                      </div>
                    )}
                  </For>
                </div>
              </Show>
            </Show>

            <Show when={tab() === "changes" && stashMode()}>
              <button class="back" onClick={() => { setStashMode(false); setStashSel(undefined); setStashEdit(undefined); }}>← Volver a Cambios</button>
              <div class="snew">
                <input placeholder="Mensaje del nuevo stash (opcional)" value={stashMsg()} onInput={(e) => setStashMsg(e.currentTarget.value)}
                  onKeyDown={(e) => e.key === "Enter" && changes()?.length && stashDo(async () => { await commands.stashPush(cur()!, stashMsg()); setStashMsg(""); })} />
                <button class="btn primary" disabled={!changes()?.length} title={changes()?.length ? "Guarda los cambios actuales y deja el árbol limpio" : "No hay cambios que guardar"}
                  onClick={() => stashDo(async () => { await commands.stashPush(cur()!, stashMsg()); setStashMsg(""); })}>Guardar cambios</button>
              </div>
              <div class="list">
                <Show when={!stashes()?.length}><p class="muted">No hay cambios guardados</p></Show>
                <For each={stashes() ?? []}>
                  {(st) => (
                    <div class="li commit stashrow" classList={{ sel: stashSel() === st.index }} onClick={() => setStashSel(st.index)}>
                      <Show when={stashEdit()?.index === st.index} fallback={
                        <>
                          <b class="ellip" title={st.message}>{st.message}</b>
                          <span class="meta"><small class="ellip">stash@{"{"}{st.index}{"}"} · {when(st.time)}</small></span>
                          <span class="acts">
                            <button class="ibtn on" title="Editar el mensaje" onClick={(e) => { e.stopPropagation(); setStashEdit({ index: st.index, msg: st.message.replace(/^(WIP on|On) [^:]+: /, "") }); }}><Icon name="pencil" size={14} /></button>
                            <button class="ibtn danger on" title="Eliminar" onClick={(e) => { e.stopPropagation(); askDropStash(st); }}><Icon name="trash" size={14} /></button>
                          </span>
                        </>
                      }>
                        <div class="sedit" onClick={(e) => e.stopPropagation()}>
                          <input autofocus value={stashEdit()!.msg} onInput={(e) => setStashEdit({ index: st.index, msg: e.currentTarget.value })}
                            onKeyDown={(e) => { if (e.key === "Enter") stashRename(); if (e.key === "Escape") { e.stopPropagation(); setStashEdit(undefined); } }} />
                          <button class="btn sm primary" onClick={stashRename}>Guardar</button>
                          <button class="btn sm" onClick={() => setStashEdit(undefined)}>Cancelar</button>
                        </div>
                      </Show>
                    </div>
                  )}
                </For>
              </div>
            </Show>

            <Show when={tab() === "changes" && !stashMode()}>
              <div class="fsearch pad">
                <span class="fbtn"><Icon name="filter" /><Icon name="triangleDown" size={12} /></span>
                <input placeholder="Filtrar" value={vis()} onInput={(e) => setVis(e.currentTarget.value)} />
              </div>
              <div class="chead">
                <input type="checkbox" checked={allOn()} ref={(el) => createEffect(() => (el.indeterminate = !allOn() && included().length > 0))} onChange={toggleAll} />
                <span class="grow">{changes.loading ? "Leyendo estado…" : `${changes()?.length ?? 0} ${changes()?.length === 1 ? "archivo modificado" : "archivos modificados"}`}</span>
                <button class="x" title="Descartar los cambios seleccionados" disabled={!included().length} onClick={() => setConfirmDiscard(true)}><Icon name="undo" size={14} /></button>
              </div>
              <div class="list">
                <For each={visible()}>
                  {(c) => (
                    <div class="li file" classList={{ sel: file() === c.path }} onClick={() => setFile(c.path)}
                      onContextMenu={(e) => { e.preventDefault(); setFctx({ x: Math.min(e.clientX, window.innerWidth - 270), y: Math.min(e.clientY, window.innerHeight - 280), path: c.path, kind: c.kind }); }}>
                      <input type="checkbox" checked={!off().has(c.path)} ref={(el) => createEffect(() => (el.indeterminate = partial().has(c.path)))} onClick={(e) => e.stopPropagation()} onChange={() => toggle(c.path)} />
                      <span class="path" title={c.path}><span class="dir">{split(c.path)[0]}</span>{split(c.path)[1]}</span>
                      <span class={`st st-${c.kind}`}><Icon name={statusIcon[c.kind] ?? "diffModified"} /></span>
                    </div>
                  )}
                </For>
              </div>
              <div class="commitbox">
                <div class="cmsg">
                  <Avatar name={ident()?.[0] || ident()?.[1] || "?"} email={ident()?.[1]} src={ghUser()?.avatar_url} />
                  <input class="field" placeholder={autoSummary() || "Resumen (obligatorio)"} value={summary()} onInput={(e) => { setSummary(e.currentTarget.value); setJustCommitted(undefined); }} />
                  <button class="ibtn ai" classList={{ busy: aiBusy() }} disabled={aiBusy() || (aiOn() && (!included().length || amend()))} onClick={() => (aiOn() ? aiMessage() : openAiDlg())}
                    title={aiOn() ? "Redactar título y descripción con IA (envía el diff al proveedor configurado)" : "IA sin configurar: clic para elegir proveedor y clave"}>
                    <Icon name="sparkles" size={16} />
                  </button>
                  <button class="ibtn" onClick={openAiDlg} title="Configuración de IA"><Icon name="settings" size={16} /></button>
                </div>
                <textarea class="field" placeholder="Descripción" rows="4" value={body()} onInput={(e) => setBody(e.currentTarget.value)} />
                <button class="btn primary full" disabled={!finalSummary() || (!included().length && !amend()) || committing()} onClick={doCommit}>
                  {committing() ? "Haciendo commit…" : amend() ? <>Actualizar último commit en <b>{branch() ?? "…"}</b></> : <>Hacer commit a <b>{branch() ?? "…"}</b></>}
                </button>
              </div>
              {/* Como GitHub Desktop: la tarjeta aparece solo tras commitear (o mientras se enmienda). */}
              <Show when={amend()}>
                <div class="undo"><Icon name="gitCommit" /><span class="grow">Editando el último commit</span><button class="btn sm" onClick={() => { toggleAmend(false); setSummary(""); setBody(""); }}>Cancelar</button></div>
              </Show>
              <Show when={!amend() && unpushed()}>
                {(m) => (
                  <div class="undo">
                    <Icon name="gitCommit" />
                    <span class="grow"><small>Último commit{(sync()?.ahead ?? 0) > 0 ? " · sin subir" : ""}</small><span class="ellip" title={m()}>{m()}</span></span>
                    <button class="btn sm" title="Cambia el mensaje o añade archivos al último commit, en vez de crear uno nuevo. Úsalo solo si aún no hiciste push." onClick={() => toggleAmend(true)}>Editar</button>
                    <button class="btn sm" onClick={undo}>Deshacer</button>
                  </div>
                )}
              </Show>
            </Show>

            <Show when={tab() === "history"}>
              <div class="hfilter">
                <div class="hrev">
                  <Icon name="gitBranch" size={14} />
                  <select value={fRev()} onChange={(e) => setFRev(e.currentTarget.value)} title="Ver el historial de otra rama (para hacer cherry-pick de sus commits)">
                    <option value="">Historial de «{branch()}» (actual)</option>
                    <For each={(histBranches() ?? []).filter((b) => b.name !== branch())}>{(b) => <option value={b.name}>{b.name}</option>}</For>
                  </select>
                  <button class="btn sm" title="Ver todos los sitios donde ha estado HEAD y recuperar commits perdidos" onClick={() => setReflogOpen(true)}>Recuperar…</button>
                  <Show when={fRev()}><label class="opt-check inline" title={`Oculta lo que «${branch()}» ya contiene`}><input type="checkbox" checked={fOnlyNew()} onChange={(e) => setFOnlyNew(e.currentTarget.checked)} /> Solo lo que falta en «{branch()}»</label></Show>
                </div>
                <div class="fsearch hsearch"><Icon name="search" /><input placeholder="Buscar en los mensajes" value={fSearch()} onInput={(e) => onSearch(e.currentTarget.value)} /></div>
                <Show when={fFile()}><span class="chip" title={fFile()}><Icon name="history" size={12} /><span class="ellip">{fFile().split("/").pop()}</span><button class="x" title="Quitar el filtro de archivo" onClick={() => setFFile("")}><Icon name="x" size={12} /></button></span></Show>
                <select value={fAuthor()} onChange={(e) => setFAuthor(e.currentTarget.value)} title="Filtrar por colaborador">
                  <option value="">Todos los colaboradores</option>
                  <For each={authors() ?? []}>{(a) => <option value={a.email}>{a.name} ({a.commits})</option>}</For>
                </select>
                <input type="date" value={fSince()} max={fUntil() || undefined} onChange={(e) => setFSince(e.currentTarget.value)} title="Desde" />
                <input type="date" value={fUntil()} min={fSince() || undefined} onChange={(e) => setFUntil(e.currentTarget.value)} title="Hasta" />
                <Show when={filtered()}><button class="x" title="Quitar filtros" onClick={clearFilters}><Icon name="x" size={14} /></button></Show>
              </div>
              <Show when={fRev()}><div class="histhint"><Icon name="cherry" size={14} /><span>Viendo «{fRev()}». Clic derecho en un commit y <b>Cherry-pick</b> para copiarlo a «{branch()}».</span></div></Show>
              <div class="list" onScroll={onHistScroll}>
                <Show when={!commits().length}><p class="muted">{histBusy() ? "Cargando…" : (filtered() ? "Ningún commit coincide con el filtro" : "Aún no hay commits")}</p></Show>
                <For each={commits()}>
                  {(c) => (
                    <div class="li commit" classList={{ sel: sel() === c.id }} onClick={() => setSel(c.id)}
                      onContextMenu={(e) => { e.preventDefault(); setCctx({ x: Math.min(e.clientX, window.innerWidth - 270), y: Math.min(e.clientY, window.innerHeight - 290), id: c.id, summary: c.summary }); }}>
                      <b class="ellip">{c.summary}</b>
                      <Show when={tagsOf(c.id).length}><span class="tags"><For each={tagsOf(c.id)}>{(t) => <span class="tagb" title={`Tag ${t.name}`}><Icon name="tag" size={11} />{t.name}</span>}</For></span></Show>
                      <span class="meta"><Avatar name={c.author} email={c.email} size={16} /><small class="ellip">{c.author} · {when(c.time)}</small></span>
                    </div>
                  )}
                </For>
              </div>
            </Show>
          </aside>
          <div class="resizer" onMouseDown={startResize} title="Arrastra para cambiar el ancho" />

          <main class="main">
            {/* ---- Historial: detalle del commit ---- */}
            <Show when={tab() === "history"}>
              <Show when={detail()} keyed fallback={<p class="muted">Selecciona un commit para ver sus cambios</p>}>
                {(d) => (
                  <>
                    <div class="chdr">
                      <div class="grow">
                        <h2 class="ellip" title={d.summary}>{d.summary}</h2>
                        <div class="cmeta">
                          <Avatar name={d.author} email={d.email} size={16} /><span>{d.author}</span>
                          <Icon name="gitCommit" /><span class="mono">{d.id.slice(0, 7)}</span>
                          <button class="x" title="Copiar el hash completo" onClick={() => navigator.clipboard?.writeText(d.id)}><Icon name="copy" size={14} /></button>
                          <small>{when(d.time)}</small>
                        </div>
                        <Show when={d.body}>
                          <pre class="cbody" classList={{ clamp: !bodyOpen() && (d.body.length > 400 || d.body.split("\n").length > 6) }}>{d.body}</pre>
                          <Show when={d.body.length > 400 || d.body.split("\n").length > 6}>
                            <button class="link" onClick={() => setBodyOpen(!bodyOpen())}>{bodyOpen() ? "Mostrar menos" : "Mostrar más"}</button>
                          </Show>
                        </Show>
                      </div>
                      <span class="stats"><span class="plus">+{d.additions}</span> <span class="minus">−{d.deletions}</span></span>
                    </div>
                    <div class="split">
                      <div class="cfiles">
                        <div class="chead center"><span>{d.files.length} {d.files.length === 1 ? "archivo modificado" : "archivos modificados"}</span></div>
                        <div class="list">
                          <For each={d.files}>
                            {(f) => (
                              <div class="li file" classList={{ sel: selFile() === f.path }} onClick={() => setSelFile(f.path)}>
                                <span class="path" title={f.path}><span class="dir">{split(f.path)[0]}</span>{split(f.path)[1]}</span>
                                <span class={`st st-${f.kind}`}><Icon name={statusIcon[f.kind] ?? "diffModified"} /></span>
                              </div>
                            )}
                          </For>
                        </div>
                      </div>
                      <div class="cdiff">
                        <Show when={selFile()}><div class="dhead"><span class="path"><span class="dir">{split(selFile()!)[0]}</span><b>{split(selFile()!)[1]}</b></span></div></Show>
                        <Show when={cdiff.error}><p class="err">{String(cdiff.error)}</p></Show>
                        <Show when={cdiff() !== undefined && !cdiff.loading}>
                          <Show when={!isBinary(cdiff()!)} fallback={<p class="muted">Archivo binario: no hay diff de texto.</p>}>
                            <DiffView text={cdiff()!} />
                          </Show>
                        </Show>
                      </div>
                    </div>
                  </>
                )}
              </Show>
            </Show>

            {/* ---- Cambios: diff o estado vacío ---- */}
            <Show when={tab() === "prs"}>
              <Show when={prd()} keyed fallback={<p class="muted">{prd.loading ? "Cargando…" : "Selecciona un Pull Request para revisarlo"}</p>}>
                {(d) => (
                  <>
                    <div class="chdr">
                      <div class="grow">
                        <h2 class="ellip" title={d.pr.title}>{d.pr.title} <span class="muted-n">#{d.pr.number}</span></h2>
                        <div class="cmeta">
                          <span class={`pill ${d.pr.draft && d.pr.state === "open" ? "draft" : d.pr.state}`}>{prBadge(d.pr.state, d.pr.draft)}</span>
                          <Avatar name={d.pr.author} src={d.pr.avatar_url} size={16} /><span>{d.pr.author}</span>
                          <Icon name="gitBranch" size={14} /><span class="mono">{d.pr.head} → {d.pr.base}</span>
                          <small>{d.commits} {d.commits === 1 ? "commit" : "commits"} · {isoAgo(d.pr.updated_at)}</small>
                        </div>
                        <Show when={d.pr.state === "open"}>
                          <p class={`mstate ${d.mergeable === false ? "bad" : ""}`}>
                            {d.mergeable === false ? "Tiene conflictos con la rama base: resuélvelos antes de fusionar." : d.mergeable_state === "blocked" ? "Fusión bloqueada (revisiones o checks pendientes en GitHub)." : d.mergeable_state === "behind" ? "La rama está por detrás de la base." : d.mergeable === null ? "GitHub aún calcula si se puede fusionar…" : "Lista para fusionar."}
                          </p>
                        </Show>
                        <Show when={d.body}>
                          <pre class="cbody" classList={{ clamp: !prBodyOpen() && (d.body.length > 400 || d.body.split("\n").length > 6) }}>{d.body}</pre>
                          <Show when={d.body.length > 400 || d.body.split("\n").length > 6}>
                            <button class="link" onClick={() => setPrBodyOpen(!prBodyOpen())}>{prBodyOpen() ? "Mostrar menos" : "Mostrar más"}</button>
                          </Show>
                        </Show>
                      </div>
                      <span class="sact col">
                        <button class="btn primary" disabled={prBusy()} title="Descarga el PR (también de forks) y cambia a la rama pr-N" onClick={() => prCheckout(d.pr.number)}>Hacer checkout</button>
                        <Show when={d.pr.state === "open"}>
                          <span class="mrow">
                            <select value={mergeMethod()} onChange={(e) => setMergeMethod(e.currentTarget.value as "merge" | "squash" | "rebase")}>
                              <option value="merge">Merge commit</option><option value="squash">Squash</option><option value="rebase">Rebase</option>
                            </select>
                            <Show when={mergeAsk()} fallback={<button class="btn" disabled={prBusy() || d.mergeable === false} onClick={() => { setCloseAsk(false); setMergeAsk(true); }}>Fusionar</button>}>
                              <button class="btn primary" disabled={prBusy()} onClick={() => prMerge(d.pr.number)}>Sí, fusionar</button>
                            </Show>
                          </span>
                          <Show when={mergeAsk()}><button class="btn" onClick={() => setMergeAsk(false)}>No fusionar</button></Show>
                          <Show when={closeAsk()} fallback={<button class="btn danger" disabled={prBusy()} onClick={() => { setMergeAsk(false); setCloseAsk(true); }}>Cerrar PR</button>}>
                            <button class="btn danger" disabled={prBusy()} onClick={() => prClose(d.pr.number)}>Sí, cerrar</button>
                          </Show>
                          <Show when={closeAsk()}><button class="btn" onClick={() => setCloseAsk(false)}>No cerrar</button></Show>
                        </Show>
                        <button class="btn" onClick={() => openUrl(d.pr.url)}>Abrir en GitHub</button>
                      </span>
                    </div>
                    <Show when={checks()?.length}>
                      <div class="checks">
                        <b class={checks()!.some((c) => c.state === "failure") ? "bad" : checks()!.some((c) => c.state === "pending") ? "wait" : "ok"}>{checksSummary()}</b>
                        <For each={checks()}>{(c) => <button class={`chk-pill ${c.state}`} title={c.name} onClick={() => c.url && openUrl(c.url)}>{c.name}</button>}</For>
                      </div>
                    </Show>
                    <div class="subtabs">
                      <button classList={{ on: prView() === "files" }} onClick={() => setPrView("files")}>Archivos ({d.files.length})</button>
                      <button classList={{ on: prView() === "talk" }} onClick={() => setPrView("talk")}>Conversación{comments()?.length ? ` (${comments()!.length})` : ""}</button>
                    </div>
                    <Show when={prView() === "talk"}>
                      <div class="talk">
                        <Show when={comments.loading && !comments()}><p class="muted">Cargando…</p></Show>
                        <Show when={comments() && !comments()!.length}><p class="muted">Aún no hay comentarios</p></Show>
                        <For each={comments() ?? []}>
                          {(c) => (
                            <div class="cmt">
                              <Avatar name={c.author} src={c.avatar_url} size={24} />
                              <div class="grow"><div><b>{c.author}</b> <small class="muted-n">{isoAgo(c.created_at)}</small></div><pre class="cbody">{c.body}</pre></div>
                            </div>
                          )}
                        </For>
                        <div class="newc">
                          <textarea class="ta" rows="3" placeholder="Escribe un comentario…" value={newComment()} onInput={(e) => setNewComment(e.currentTarget.value)} />
                          <button class="btn primary" disabled={prBusy() || !newComment().trim()} onClick={() => sendComment(d.pr.number)}>Comentar</button>
                        </div>
                      </div>
                    </Show>
                    <Show when={prView() === "files"}>
                    <div class="split">
                      <div class="cfiles">
                        <div class="chead center"><span>{d.files.length} {d.files.length === 1 ? "archivo" : "archivos"} · <span class="plus">+{d.files.reduce((a, f) => a + f.additions, 0)}</span> <span class="minus">−{d.files.reduce((a, f) => a + f.deletions, 0)}</span></span></div>
                        <div class="list">
                          <For each={d.files}>
                            {(f) => (
                              <div class="li file" classList={{ sel: prFile() === f.path }} onClick={() => setPrFile(f.path)}>
                                <span class="path" title={f.path}><span class="dir">{split(f.path)[0]}</span>{split(f.path)[1]}</span>
                                <span class={`st st-${prStatusKind(f.status)}`}><Icon name={statusIcon[prStatusKind(f.status)] ?? "diffModified"} /></span>
                              </div>
                            )}
                          </For>
                        </div>
                      </div>
                      <div class="cdiff">
                        <Show when={prFile()}><div class="dhead"><span class="path"><span class="dir">{split(prFile()!)[0]}</span><b>{split(prFile()!)[1]}</b></span></div></Show>
                        <Show when={prPatch() !== null} fallback={<Show when={prFile()}><p class="muted">Sin diff de texto (binario o demasiado grande). Ábrelo en GitHub.</p></Show>}>
                          <DiffView text={prPatch()!} />
                        </Show>
                      </div>
                    </div>
                    </Show>
                  </>
                )}
              </Show>
            </Show>

            <Show when={tab() === "changes" && stashMode()}>
              <Show when={stashCur()} keyed fallback={<p class="muted">Selecciona un stash para ver qué contiene</p>}>
                {(st) => (
                  <>
                    <div class="chdr">
                      <div class="grow">
                        <h2 class="ellip" title={st.message}>{st.message}</h2>
                        <div class="cmeta"><Icon name="archive" size={14} /><span class="mono">stash@{"{"}{st.index}{"}"}</span><small>{when(st.time)}</small></div>
                      </div>
                      <span class="sact">
                        <button class="btn" title="Restaura los cambios y borra el stash" onClick={() => stashDo(() => commands.stashOp(cur()!, st.index, "pop"))}>Restaurar</button>
                        <button class="btn" title="Restaura y conserva el stash" onClick={() => stashDo(() => commands.stashOp(cur()!, st.index, "apply"))}>Aplicar</button>
                        <button class="btn danger" onClick={() => askDropStash(st)}>Eliminar</button>
                      </span>
                    </div>
                    <div class="split">
                      <div class="cfiles">
                        <div class="chead center"><span>{stFiles()?.length ?? 0} {stFiles()?.length === 1 ? "archivo" : "archivos"}</span></div>
                        <div class="list">
                          <Show when={stFiles.loading}><p class="muted">Cargando…</p></Show>
                          <For each={stFiles() ?? []}>
                            {(f) => (
                              <div class="li file" classList={{ sel: stashFile() === f.path }} onClick={() => setStashFile(f.path)}>
                                <span class="path" title={f.path}><span class="dir">{split(f.path)[0]}</span>{split(f.path)[1]}</span>
                                <span class={`st st-${f.kind}`}><Icon name={statusIcon[f.kind] ?? "diffModified"} /></span>
                              </div>
                            )}
                          </For>
                        </div>
                      </div>
                      <div class="cdiff">
                        <Show when={stashFile()}><div class="dhead"><span class="path"><span class="dir">{split(stashFile()!)[0]}</span><b>{split(stashFile()!)[1]}</b></span></div></Show>
                        <Show when={stDiff.error}><p class="err">{String(stDiff.error)}</p></Show>
                        <Show when={stDiff() !== undefined && !stDiff.loading}>
                          <Show when={!isBinary(stDiff()!)} fallback={<p class="muted">Archivo binario: no hay diff de texto.</p>}>
                            <DiffView text={stDiff()!} />
                          </Show>
                        </Show>
                      </div>
                    </div>
                  </>
                )}
              </Show>
            </Show>

            <Show when={tab() === "changes" && !stashMode()}>
              <Show
                when={file()}
                fallback={
                  <div class="interstitial">
                    <Show when={emptyChanges()} fallback={<div class="ihead"><div><h1>Selecciona un archivo</h1><p>Elige un archivo de la lista para ver sus cambios.</p></div></div>}>
                      <div class="ihead">
                        <div>
                          <h1>No hay cambios locales</h1>
                          <p>No hay cambios sin commitear en este repositorio. Algunas sugerencias de qué hacer ahora.</p>
                        </div>
                        <img src="/grove-logo.png" class="blank" alt="Grove" />
                      </div>
                      <Show when={syncAct().kind === "push" || syncAct().kind === "pull"}>
                        <div class="nudge">
                          <span class="nicon"><Icon name={syncAct().icon} size={22} /></span>
                          <div class="grow">
                            <b>{syncAct().kind === "pull" ? "Hay novedades en origin" : sync()?.upstream ? "Tus commits están listos para subir" : "Esta rama aún no está en GitHub"}</b>
                            <small>{syncAct().sub}</small>
                          </div>
                          <button class="btn primary" disabled={fetching()} onClick={() => doSync()}>{syncAct().title}</button>
                        </div>
                      </Show>
                      <div class="actions">
                        <Show when={!ghUser()}>
                          <div class="act">
                            <div><b>Inicia sesión en GitHub</b><small>Para ver las fotos de los autores y, más adelante, hacer push, pull y abrir Pull Requests</small></div>
                            <button class="btn primary" onClick={ghLogin} disabled={!!ghCode()}>Iniciar sesión</button>
                          </div>
                        </Show>
                        <div class="act">
                          <div><b>Abrir el repositorio en tu editor externo</b><small>Se usa el primero que se encuentre: VS Code, Cursor, Zed o Sublime Text</small></div>
                          <button class="btn" onClick={() => launch("Editor")}>Abrir en el editor</button>
                        </div>
                        <div class="act">
                          <div><b>Ver los archivos del repositorio en {isMac ? "Finder" : "el explorador de archivos"}</b></div>
                          <button class="btn" onClick={() => launch("Folder")}>{isMac ? "Mostrar en Finder" : "Mostrar en el explorador"}</button>
                        </div>
                        <div class="act">
                          <div><b>Abrir una terminal en el repositorio</b></div>
                          <button class="btn" onClick={() => launch("Terminal")}>Abrir terminal</button>
                        </div>
                      </div>
                    </Show>
                  </div>
                }
              >
                <div class="dhead">
                  <span class="path grow"><span class="dir">{split(file()!)[0]}</span><b>{split(file()!)[1]}</b></span>
                  <Show when={!isBinary(diff() ?? "")}>
                    <button class="btn sm" onClick={() => setSideBySide(!sideBySide())} title={sideBySide() ? "Las líneas solo se eligen en la vista unificada" : ""}>
                      {sideBySide() ? "Vista unificada" : "Lado a lado"}
                    </button>
                  </Show>
                </div>
                <Show when={diff.error}><p class="err">{String(diff.error)}</p></Show>
                <Show when={diff.loading}><p class="muted">Cargando diff…</p></Show>
                <Show when={!diff.loading && diff() !== undefined}>
                  <Show
                    when={!(isBinary(diff()!) || (IMG.test(file()!) && !diff()!.trim()))}
                    fallback={
                      <Show when={IMG.test(file()!)} fallback={<p class="muted">Archivo binario: no hay diff de texto.</p>}>
                        <ImageView repo={cur()!} file={file()!} />
                      </Show>
                    }
                  >
                    <Show when={sideBySide()} fallback={<DiffView text={diff()!} checked={lineChecked} onToggle={toggleLine} hunk={hunkState} onHunk={toggleHunk} />}>
                      <SplitView text={diff()!} />
                    </Show>
                  </Show>
                </Show>
              </Show>
            </Show>
          </main>
        </div>
      </Show>
    </div>
  );
}
