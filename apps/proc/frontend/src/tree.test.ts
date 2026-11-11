import { expect, test } from "bun:test";

import { filterProcesses, orderProcesses, type Process } from "./tree";

function process(pid: number, ppid: number, name = `p${pid}`): Process {
  return {
    pid,
    ppid,
    name,
    user: "root",
    state: "sleeping",
    "cpu-percent": 0,
    "memory-bytes": 0,
    "start-ticks": 0,
  };
}

test("flat mode sorts with the default pid comparator", () => {
  const rows = orderProcesses([process(2, 1), process(1, 0)], false, new Set());
  expect(rows.map((row) => row.process.pid)).toEqual([1, 2]);
  expect(rows.every((row) => row.depth === 0)).toBe(true);
  expect(rows.every((row) => row.guides.length === 0)).toBe(true);
});

test("tree mode puts parents before sorted children", () => {
  const rows = orderProcesses(
    [process(3, 1), process(2, 1), process(1, 0)],
    true,
    new Set(),
  );
  expect(rows.map((row) => row.process.pid)).toEqual([1, 2, 3]);
  expect(rows.map((row) => row.depth)).toEqual([0, 1, 1]);
  expect(rows[0].hasChildren).toBe(true);
  expect(rows[1].hasChildren).toBe(false);
});

test("a missing parent is a root", () => {
  const rows = orderProcesses([process(7, 99)], true, new Set());
  expect(rows.map((row) => row.process.pid)).toEqual([7]);
  expect(rows[0].depth).toBe(0);
});

test("collapsed hides descendants but keeps the parent", () => {
  const rows = orderProcesses(
    [process(1, 0), process(2, 1), process(3, 2)],
    true,
    new Set([1]),
  );
  expect(rows.map((row) => row.process.pid)).toEqual([1]);
  expect(rows[0].hasChildren).toBe(true);
});

test("a parent cycle terminates", () => {
  const rows = orderProcesses(
    [process(10, 11), process(11, 10)],
    true,
    new Set(),
  );
  expect(rows.map((row) => row.process.pid).sort()).toEqual([10, 11]);
});

test("guides continue only under ancestors that are not the last sibling", () => {
  const rows = orderProcesses(
    [process(1, 0), process(2, 1), process(5, 0), process(6, 5)],
    true,
    new Set(),
  );
  expect(rows.map((row) => row.process.pid)).toEqual([1, 2, 5, 6]);
  expect(rows[1].guides).toEqual([true]);
  expect(rows[1].isLast).toBe(true);
  expect(rows[3].guides).toEqual([false]);
});

test("siblings mark the last child", () => {
  const rows = orderProcesses(
    [process(1, 0), process(2, 1), process(3, 1)],
    true,
    new Set(),
  );
  expect(rows[1].isLast).toBe(false);
  expect(rows[2].isLast).toBe(true);
});

test("a comparator reorders flat rows and tree siblings", () => {
  const byName = (left: Process, right: Process): number =>
    left.name < right.name ? -1 : left.name > right.name ? 1 : 0;
  const flat = orderProcesses(
    [process(1, 0, "b"), process(2, 0, "a")],
    false,
    new Set(),
    byName,
  );
  expect(flat.map((row) => row.process.name)).toEqual(["a", "b"]);

  const tree = orderProcesses(
    [process(1, 0, "root"), process(2, 1, "b"), process(3, 1, "a")],
    true,
    new Set(),
    byName,
  );
  expect(tree.map((row) => row.process.name)).toEqual(["root", "a", "b"]);
});

test("column filters match pid, user and state", () => {
  const rows = filterProcesses(
    [process(1, 0, "init"), process(42, 1, "sleep"), process(43, 1, "bash")],
    { pid: "42", user: "root" },
    false,
  );
  expect(rows.map((row) => row.pid)).toEqual([42]);
});

test("the mobile query matches any field", () => {
  const rows = filterProcesses(
    [process(1, 0, "init"), process(42, 1, "sleep")],
    { query: "42" },
    false,
  );
  expect(rows.map((row) => row.pid)).toEqual([42]);
});

test("tree filters keep the ancestors of a match", () => {
  const rows = filterProcesses(
    [process(1, 0, "init"), process(2, 1, "bash"), process(3, 2, "sleep")],
    { name: "sleep" },
    true,
  );
  expect(rows.map((row) => row.pid)).toEqual([1, 2, 3]);
});
