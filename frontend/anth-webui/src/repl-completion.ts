/**
 * Tab completion for the browser prompt.
 *
 * The backend derives its candidates from clap, so a command it gains is
 * completable at once. There is no clap here, so the grammar is written out —
 * but written out *once*, as data, and the parser in `repl-command.ts` accepts
 * exactly what this offers. A test holds the two together: every candidate the
 * prompt can insert parses, and every verb the parser accepts is offered.
 *
 * Completion is static for the same reason the backend's is: the only dynamic
 * candidate is a plugin id, and fetching it would make one keystroke cost a
 * request to a daemon that may be down.
 */

/** One thing Tab can insert. */
export interface Candidate {
  text: string;
  kind: "command" | "flag";
}

/** The root verbs, in the order `help` prints them. */
const ROOT: readonly string[] = ["help", "quit", "exit", "host-info", "plugin"];

/** What follows `plugin`. */
const PLUGIN: readonly string[] = ["list", "call"];

/** What `plugin call` accepts after the id. */
const CALL_FLAGS: readonly string[] = ["--http", "--direct"];

/**
 * Every candidate for the word being completed.
 *
 * `words` is the line split on whitespace with the word under the cursor as its
 * last element, so `["plugin", "li"]` completes the `li` against `list`. The
 * words before it select which part of the grammar applies, which is why an
 * unknown path offers nothing rather than the whole vocabulary.
 */
export function candidates(words: readonly string[]): Candidate[] {
  if (words.length === 0) {
    return [];
  }
  const prefix = words[words.length - 1];
  const path = words.slice(0, -1);

  const offered = grammar(path);
  if (offered === undefined) {
    return [];
  }

  return offered
    .filter((word) => word.startsWith(prefix))
    .map((word) => ({
      text: word,
      kind: word.startsWith("-") ? ("flag" as const) : ("command" as const),
    }))
    .sort((left, right) => left.text.localeCompare(right.text));
}

/**
 * The words that may follow `path`, or `undefined` past a word that names no
 * subcommand.
 *
 * The shape follows the CLI's grammar, which is clap's: a flag belongs to the
 * command, not to a position, so the pipe flags are offered after `call` and
 * before the id. `plugin call hello <Tab>` offers nothing because `hello` is a
 * positional, and clap has no grammar past a positional with no subcommands.
 *
 * Two deliberate omissions, both because this prompt is narrower than the CLI.
 * The root's own flags (`--socket`, `--addr`) are not offered: the browser
 * console has no socket to point at, so completing one would complete a line
 * it cannot run. A plugin id is not offered either: that is a request per
 * keystroke to a daemon that may be down, which is the same reason the backend
 * leaves it out.
 */
function grammar(path: readonly string[]): readonly string[] | undefined {
  const [first, second, third] = path;

  if (first === undefined) {
    return ROOT;
  }
  if (first !== "plugin") {
    return undefined;
  }
  if (second === undefined) {
    return PLUGIN;
  }
  if (second === "call" && third === undefined) {
    return CALL_FLAGS;
  }
  return undefined;
}

/** What Tab did, so the element knows whether to redraw the input. */
export interface Completion {
  /** The line after completing, unchanged when nothing could be inserted. */
  line: string;
  /** The candidates to list, when several matched and none narrowed the word. */
  matches: string[];
}

/**
 * Completes the line typed so far.
 *
 * One match is inserted with a trailing space, so the next Tab starts a new
 * word instead of extending the one just completed. Several matches insert the
 * prefix they share, and list themselves only when even that makes no progress:
 * that is the second-Tab behaviour of the backend prompt, reduced to what a
 * single Tab has to decide.
 */
export function completeLine(text: string): Completion {
  const words = splitWords(text);
  const found = candidates(words);
  if (found.length === 0) {
    return { line: text, matches: [] };
  }

  const stem = words[words.length - 1];
  const head = text.slice(0, text.length - stem.length);

  if (found.length === 1) {
    return { line: `${head}${found[0].text} `, matches: [] };
  }

  const shared = commonPrefix(found.map((candidate) => candidate.text));
  if (shared.length > stem.length) {
    return { line: `${head}${shared}`, matches: [] };
  }
  return { line: text, matches: found.map((candidate) => candidate.text) };
}

/**
 * The words of a line, with an empty last word after a trailing space.
 *
 * That empty word is what makes `plugin ` complete the subcommand rather than
 * re-completing `plugin`, so the split cannot simply drop empty strings.
 */
export function splitWords(text: string): string[] {
  if (text.length === 0) {
    return [""];
  }
  const words = text.split(/\s+/);
  if (!/\s$/.test(text)) {
    return words;
  }
  return [...words.filter((word) => word.length > 0), ""];
}

function commonPrefix(words: readonly string[]): string {
  if (words.length === 0) {
    return "";
  }
  let prefix = words[0];
  for (const word of words.slice(1)) {
    while (!word.startsWith(prefix)) {
      prefix = prefix.slice(0, -1);
      if (prefix.length === 0) {
        return "";
      }
    }
  }
  return prefix;
}
