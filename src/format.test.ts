// Run: node --test src/format.test.ts (Node >= 22.18 strips types). Excluded from tsc/vite (no node types).
import test from "node:test";
import assert from "node:assert/strict";
import { relativeTime, updatingLabel } from "./format.ts";

test("relativeTime: under one minute", () => {
  assert.equal(relativeTime(0), "1分未満");
  assert.equal(relativeTime(59), "1分未満");
});

test("relativeTime: minute boundaries floor", () => {
  assert.equal(relativeTime(60), "1分前");
  assert.equal(relativeTime(119), "1分前");
  assert.equal(relativeTime(120), "2分前");
  assert.equal(relativeTime(180), "3分前");
  assert.equal(relativeTime(239), "3分前");
  assert.equal(relativeTime(240), "4分前");
});

test("relativeTime: file_mtime source is prefixed with ~, event is not", () => {
  assert.equal(relativeTime(180, "file_mtime"), "~3分前");
  assert.equal(relativeTime(59, "file_mtime"), "~1分未満");
  assert.equal(relativeTime(180, "event"), "3分前");
});

test("updatingLabel: empty when not updating, regardless of confidence", () => {
  assert.equal(updatingLabel(false), "");
  assert.equal(updatingLabel(false, "low"), "");
  assert.equal(updatingLabel(false, "high"), "");
});

test("updatingLabel: low confidence adds question mark", () => {
  assert.equal(updatingLabel(true), "更新中");
  assert.equal(updatingLabel(true, "high"), "更新中");
  assert.equal(updatingLabel(true, "low"), "更新中?");
});
