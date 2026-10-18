// Requires Playwright (or NODE_PATH pointing to a bundled installation).
import { createRequire } from "node:module";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { build } from "vite";
const { chromium } = createRequire(import.meta.url)("playwright");
const browser = await chromium.launch({ headless: true, channel: process.env.LENS_TEST_BROWSER });
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const testEntry = `
    import React from 'react';
    import { createRoot } from 'react-dom/client';
    import { mockIPC } from '@tauri-apps/api/mocks';
    import { emit } from '@tauri-apps/api/event';
    import App from '/src/App.tsx';
    import '/src/index.css';
    window.isTauri = true;
    window.tokenCalls = [];
    const project = { key:'repo', name:'Token Test Project', origin:{kind:'windows'}, lastUpdatedAt:'2026-10-18T12:00:00Z',
      lastUpdatedSource:'event', updating:false, originAvailable:true, agents:['codex','claude_code','agy'], sessionUids:['new','old','unknown','error'] };
    const session = (id, agent, time) => ({sessionUid:id, nativeId:id, agent, sourceId:agent+'-local', origin:{kind:'windows'},
      lastActivityAt:time, lastActivitySource:'event', projects:[],
      status:{value:'idle',reason:'turn completed',confidence:'high',stale:false,autoReview:false},
      currentTask:{title:id+' session',lastUserMessage:'Example request',lastAssistantMessage:'Example answer'},
      hierarchy:{childUids:[],kind:'top_level'},health:{completeness:'complete',diagnostics:[]}});
    const sessions = [session('old','codex','2026-10-17T12:00:00Z'),session('new','codex','2026-10-18T12:00:00Z'),
      session('unknown','claude_code','2026-10-17T11:00:00Z'),session('error','agy','2026-10-17T10:00:00Z'),
      session('partial','agy','2026-10-17T09:00:00Z')];
    window.fail = true;
    window.updateSession = async () => { sessions[1].lastActivityAt='2026-10-18T12:01:00Z'; await emit('project_detail_changed',{key:'repo'}); };
    window.addLatest = async () => { sessions.push(session('newest','codex','2026-10-18T12:02:00Z')); await emit('project_detail_changed',{key:'repo'}); };
    window.updateChild = async () => { window.childUpdated=true; await emit('project_detail_changed',{key:'other-project'}); };
    mockIPC(async (cmd, args) => {
      if(cmd==='list_projects') return [project];
      if(cmd==='get_health') return [];
      if(cmd==='get_project_detail') return {project,sessions};
      if(cmd==='get_session_tokens') {
        window.tokenCalls.push(args.sessionUid);
        await new Promise(r=>setTimeout(r,30));
        if(args.sessionUid==='unknown') return {session:null,includingSubagents:null,subagentCount:0,unknownCount:1,failedCount:0};
        if(args.sessionUid==='error' && window.fail) throw '計算・保存に失敗しました';
        const usage = {cachedInputTokens:1000,uncachedInputTokens:200,cacheCreationInputTokens:50,outputTokens:300,totalTokens:1550};
        const report = {session:usage,includingSubagents:usage,subagentCount:0,unknownCount:0,failedCount:0};
        if(args.sessionUid==='new') {
          const factor=window.childUpdated ? 1.4 : 1.3;
          report.includingSubagents=Object.fromEntries(Object.entries(usage).map(([k,v])=>[k,Math.round(v*factor)]));
          report.subagentCount=2;
        }
        if(args.sessionUid==='partial') { report.subagentCount=2; report.unknownCount=1; report.failedCount=1; }
        return report;
      }
      throw new Error('Unexpected command: '+cmd);
    }, {shouldMockEvents:true});
    createRoot(document.getElementById('root')).render(React.createElement(App));
  `;
  const outDir = path.resolve("target/ui-session-tokens");
  await build({ logLevel: "silent", build: { outDir, emptyOutDir: true }, plugins: [{
    name: "session-token-fixture", enforce: "pre",
    load(id) { if (id.replaceAll("\\", "/").endsWith("/src/main.tsx")) return testEntry; },
  }] });
  // Serve the built app directly through browser interception: no network or live logs.
  await page.route("**/*", async (route) => {
    const url = new URL(route.request().url());
    if (url.hostname !== "lens-test.invalid") return route.abort();
    const name = url.pathname === "/" ? "index.html" : url.pathname.slice(1);
    const contentType = name.endsWith(".html") ? "text/html" : name.endsWith(".css") ? "text/css" : "application/javascript";
    await route.fulfill({ contentType, body: await readFile(path.join(outDir, name)) });
  });
  await page.goto("http://lens-test.invalid/");
  await page.getByText("Token Test Project", { exact: true }).click();
  const card = (title) => page.locator("details").filter({ has: page.locator("summary > div", { hasText: title }) }).last();
  const latest = card("new session");
  const old = card("old session");
  await latest.getByText("1,550", { exact: true }).waitFor();
  await latest.getByText("2,015", { exact: true }).waitFor();
  await latest.getByText("子・孫以降 2 セッションを含む", { exact: true }).waitFor();
  assert.equal(await latest.getAttribute("open"), "");
  assert.equal(await old.getAttribute("open"), null);
  assert.deepEqual(await page.evaluate(() => window.tokenCalls), ["new"]);
  await old.locator("summary").click();
  await old.getByText("1,550", { exact: true }).waitFor();
  assert.deepEqual(await page.evaluate(() => window.tokenCalls), ["new", "old"]);
  await old.locator("summary").click();
  assert.equal(await old.getAttribute("open"), null);
  // A refresh must preserve a user's collapsed state, and update only the open card.
  await page.evaluate(() => window.updateSession());
  await page.waitForFunction(() => window.tokenCalls.filter(x => x === "new").length === 2);
  assert.equal(await old.getAttribute("open"), null);
  await page.evaluate(() => window.updateChild());
  await latest.getByText("2,170", { exact: true }).waitFor();
  assert.equal(await old.getAttribute("open"), null);
  const unknown = card("unknown session");
  await unknown.locator("summary").click();
  await unknown.getByText("不明（ログに使用量の記録がありません）").waitFor();
  await unknown.getByRole("button", { name: "再試行" }).click();
  await page.waitForFunction(() => window.tokenCalls.filter(x => x === "unknown").length === 2);
  const failed = card("error session");
  await failed.locator("summary").click();
  await failed.getByText("計算・保存に失敗しました").waitFor();
  await page.evaluate(() => { window.fail = false; });
  await failed.getByRole("button", { name: "再試行" }).click();
  await failed.getByText("1,550", { exact: true }).waitFor();
  const partial = card("partial session");
  await partial.locator("summary").click();
  await partial.getByText("子孫込み（判明分）", { exact: true }).waitFor();
  await partial.getByText("不明 1 件・取得失敗 1 件（合計は判明分）", { exact: true }).waitFor();
  await page.evaluate(() => window.addLatest());
  await card("newest session").getByText("1,550", { exact: true }).waitFor();
  assert.equal(await latest.getAttribute("open"), null);
  assert.equal(await old.getAttribute("open"), null);
  assert.equal(await failed.getAttribute("open"), "");
  // Compact mode fills the window with just project names and update times.
  assert.equal(await page.getByRole("heading", { name: "Agent Session Lens" }).count(), 0);
  await page.getByRole("button", { name: /ログ収集の状態/ }).click();
  const main = page.getByRole("main", { name: "プロジェクト一覧" });
  for (const width of [320, 600, 899]) {
    await page.setViewportSize({ width, height: 240 });
    assert.equal(await page.locator("aside").isVisible(), false);
    assert.equal(await page.locator("footer").isVisible(), false);
    assert.equal(await main.locator("thead").isVisible(), false);
    assert.equal(await main.locator("tbody tr:first-child td:visible").count(), 2);
    assert.deepEqual(await main.boundingBox(), { x: 0, y: 0, width, height: 240 });
    assert.equal(await main.evaluate((el) => getComputedStyle(el).padding), "4px");
    const name = main.locator("tbody tr:first-child td:first-child span").first();
    await name.evaluate((el) => { el.textContent = "very-long-project-name-".repeat(12); });
    assert.equal(await main.evaluate((el) => el.scrollWidth <= el.clientWidth), true);
    if (width === 320) await page.screenshot({ path: path.join(outDir, "compact.png") });
  }
  for (const width of [900, 1280]) {
    await page.setViewportSize({ width, height: 900 });
    assert.equal(await page.locator("aside").isVisible(), true);
    assert.equal(await page.locator("footer").isVisible(), true);
    assert.equal(await main.locator("thead").isVisible(), true);
    assert.equal(await main.locator("tbody tr:first-child td:visible").count(), 3);
    assert.equal(await failed.getAttribute("open"), "");
    assert.equal(await page.locator("#source-health-details").isVisible(), true);
  }
  await page.screenshot({ path: path.join(outDir, "normal.png") });
  assert.deepEqual(errors, []);
  console.log("PASS: session tokens and retry; compact layout at 320/600/899px, normal layout at 900/1280px, state preserved across resize");
} finally {
  await browser.close();
}
