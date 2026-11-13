// Iconos Lucide (ISC, https://lucide.dev): trazo fino y esquinas redondeadas, propios de Grove.
import { Dynamic } from "solid-js/web";
import {
  Archive, ArrowDown, Cherry, CircleQuestionMark, GitMerge, Tag, TriangleAlert, ArrowUp, Check, ChevronDown, ChevronUp, Clock, Code, Copy, FileMinus, FileOutput, FilePenLine, FilePlus,
  Folder, FolderGit2, FolderInput, GitBranch, GitCommitHorizontal, ListFilter, Lock, Pencil, Plus, RefreshCw, Search, Settings, Sparkles, SquareTerminal, Trash, Undo2, UserPlus, X,
} from "lucide-solid";

const I = {
  lock: Lock,
  plus: Plus,
  sparkles: Sparkles,
  settings: Settings,
  folderMove: FolderInput,
  repo: FolderGit2,
  fileDirectory: Folder,
  gitBranch: GitBranch,
  sync: RefreshCw,
  triangleDown: ChevronDown,
  triangleUp: ChevronUp,
  x: X,
  filter: ListFilter,
  diffAdded: FilePlus,
  diffModified: FilePenLine,
  diffRemoved: FileMinus,
  diffRenamed: FileOutput,
  search: Search,
  personAdd: UserPlus,
  history: Clock,
  gitCommit: GitCommitHorizontal,
  terminal: SquareTerminal,
  code: Code,
  undo: Undo2,
  trash: Trash,
  check: Check,
  copy: Copy,
  arrowUp: ArrowUp,
  arrowDown: ArrowDown,
  pencil: Pencil,
  archive: Archive,
  gitMerge: GitMerge,
  cherry: Cherry,
  tag: Tag,
  help: CircleQuestionMark,
  warn: TriangleAlert,
} as const;

export type IconName = keyof typeof I;

export function Icon(props: { name: IconName; size?: number; class?: string }) {
  return <Dynamic component={I[props.name]} class={`icon ${props.class ?? ""}`} size={props.size ?? 16} stroke-width={1.75} aria-hidden="true" />;
}
