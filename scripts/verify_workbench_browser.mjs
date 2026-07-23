import { createRequire } from 'node:module';
import fs from 'node:fs/promises';
import path from 'node:path';

const require = createRequire(import.meta.url);
const puppeteer = require('C:/Windows/Temp/rustplc-browser/node_modules/puppeteer-core');
const root = 'E:/personal_project/rust_plc';
const artifactDir = path.join(root, 'web-ui', 'artifacts');
const baseUrl = process.env.RUSTPLC_WORKBENCH_URL || 'http://127.0.0.1:8080';
const requestedViewport = process.env.RUSTPLC_WORKBENCH_VIEWPORT?.match(/^(\d+)x(\d+)$/);
const viewports = requestedViewport
  ? [{ width: Number(requestedViewport[1]), height: Number(requestedViewport[2]) }]
  : [{ width: 1440, height: 900 }, { width: 1920, height: 1080 }];
await fs.mkdir(artifactDir, { recursive: true });

const browser = await puppeteer.launch({
  executablePath: 'C:/Program Files/Google/Chrome/Application/chrome.exe',
  headless: true,
  args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-cache'],
});

const results = [];
async function pressControlShortcut(page, key) {
  await page.keyboard.down('Control');
  await page.keyboard.press(key);
  await page.keyboard.up('Control');
}

async function selectProject(page, projectName) {
  await page.evaluate((name) => {
    const button = [...document.querySelectorAll('.wb-project-switcher button')]
      .find((item) => item.textContent?.includes(name));
    if (!(button instanceof HTMLButtonElement)) throw new Error(`project missing: ${name}`);
    button.click();
  }, projectName);
  await page.waitForFunction((name) => document.querySelector('.wb-project-switcher button.is-selected')?.textContent?.includes(name), {}, projectName);
  await page.evaluate(() => {
    const overview = [...document.querySelectorAll('.wb-tree-leaf')]
      .find((item) => item.textContent?.includes('Project Overview'));
    if (overview instanceof HTMLButtonElement) overview.click();
  });
  await page.waitForFunction((name) => document.querySelector('.wb-view-header h1')?.textContent?.includes(name), {}, projectName);
  return page.evaluate(() => ({
    selected: document.querySelector('.wb-project-switcher button.is-selected')?.textContent?.trim(),
    heading: document.querySelector('.wb-view-header h1')?.textContent?.trim(),
  }));
}

try {
  for (const viewport of viewports) {
    const page = await browser.newPage();
    await page.setCacheEnabled(false);
    page.on('pageerror', (error) => console.error('PAGE_ERROR', error.message));
    page.on('console', (message) => {
      if (message.type() === 'error') console.error('BROWSER_CONSOLE', message.text());
    });
    page.on('response', (response) => {
      if (response.status() >= 400) console.error('HTTP_ERROR', response.status(), response.url());
    });
    await page.evaluateOnNewDocument(() => localStorage.clear());
    await page.setViewport(viewport);
    const selftestUrl = new URL(baseUrl);
    selftestUrl.searchParams.set('workbench_selftest', `${Date.now()}-${viewport.width}`);
    await page.goto(selftestUrl.toString(), { waitUntil: 'networkidle0' });
    if (await page.$('input[type="password"]')) {
      await page.type('input[type="text"]', 'electrical');
      await page.type('input[type="password"]', 'password');
      await Promise.all([
        page.waitForNavigation({ waitUntil: 'networkidle0' }),
        page.click('button[type="submit"]'),
      ]);
    }
    await page.waitForSelector('.wb-shell');
    await page.waitForSelector('.wb-project-switcher button');
    const projectLabels = await page.$$eval('.wb-project-switcher button', (buttons) => buttons.map((button) => button.textContent ?? ''));
    const requiredProjects = ['Three-station Assembly Line', 'Stepper Collision Guard', 'Dual-slot Shuttle Press Cell Complex Self-test'];
    const missingProjects = requiredProjects.filter((name) => !projectLabels.some((label) => label.includes(name)));
    if (missingProjects.length > 0) throw new Error(`required delivery projects missing: ${missingProjects.join(', ')}`);
    const projectCoverage = [];
    for (const projectName of requiredProjects) projectCoverage.push(await selectProject(page, projectName));
    const projectSurface = await page.evaluate(() => ({
      selected: document.querySelector('.wb-project-switcher button.is-selected')?.textContent,
      heading: document.querySelector('.wb-view-header h1')?.textContent,
      state: document.querySelector('.wb-state strong')?.textContent,
    }));
    if (!projectSurface.heading?.includes('Dual-slot Shuttle Press Cell')) {
      throw new Error(`canonical project surface unavailable: ${JSON.stringify(projectSurface)}`);
    }
    await page.waitForFunction(() => document.querySelectorAll('.wb-pipeline-row').length >= 10);
    await page.waitForSelector('[data-release-status]');

    const overview = await page.evaluate(() => ({
      projectCount: document.querySelectorAll('.wb-project-switcher button').length,
      pipelineRows: document.querySelectorAll('.wb-pipeline-row').length,
      holdRows: document.querySelectorAll('.wb-hold-row').length,
      releaseStatus: document.querySelector('[data-release-status]')?.getAttribute('data-release-status'),
      viewportOverflowX: document.documentElement.scrollWidth > window.innerWidth,
      viewportOverflowY: document.documentElement.scrollHeight > window.innerHeight,
      shell: (() => {
        const rect = document.querySelector('.wb-shell')?.getBoundingClientRect();
        return rect ? { width: rect.width, height: rect.height } : null;
      })(),
    }));
    await page.screenshot({ path: path.join(artifactDir, `workbench-overview-${viewport.width}x${viewport.height}.png`) });

    await page.evaluate(() => {
      const button = [...document.querySelectorAll('button')]
        .find((item) => item.textContent?.includes('Controller I/O and Point Checks'));
      if (!(button instanceof HTMLButtonElement)) throw new Error('wiring view command missing');
      button.click();
    });
    await page.waitForFunction(() => document.querySelectorAll('.wb-data-table tbody tr').length === 16);
    const wiringRows = await page.$$eval('.wb-data-table tbody tr', (rows) => rows.length);
    await page.screenshot({ path: path.join(artifactDir, `workbench-wiring-${viewport.width}x${viewport.height}.png`) });

    const pointProjectionBefore = await page.evaluate(async () => {
      const response = await fetch('/api/delivery-projects/station.dual_slot_shuttle_press_cell/physical-evidence');
      if (!response.ok) throw new Error(`physical evidence request failed: ${response.status}`);
      return response.json();
    });
    const firstPointAction = await page.$('.wb-wiring-table tbody tr .wb-icon-command');
    if (!firstPointAction) throw new Error('point observation action missing');
    await firstPointAction.click();
    await page.waitForSelector('.wb-point-dialog');
    await page.evaluate(() => {
      const button = [...document.querySelectorAll('.wb-point-dialog [aria-label="Point observation status"] button')]
        .find((item) => item.textContent?.trim() === 'blocked');
      if (!(button instanceof HTMLButtonElement)) throw new Error('blocked point status control missing');
      button.click();
    });
    const pointNote = `Automated browser validation ${viewport.width}x${viewport.height}; not physical acceptance.`;
    await page.type('.wb-point-dialog textarea', pointNote);
    const photoInput = await page.$('.wb-photo-input input[type="file"]');
    if (!photoInput) throw new Error('point photo input missing');
    await photoInput.uploadFile(path.join(artifactDir, `workbench-overview-${viewport.width}x${viewport.height}.png`));
    await page.screenshot({ path: path.join(artifactDir, `workbench-point-observation-${viewport.width}x${viewport.height}.png`) });
    await page.click('.wb-point-dialog button[type="submit"]');
    await page.waitForSelector('.wb-point-dialog', { hidden: true });
    await page.waitForFunction(() => document.querySelector('.wb-wiring-table tbody tr .wb-latest-observation small')?.textContent?.includes('blocked'));
    const pointProjectionAfter = await page.evaluate(async () => {
      const response = await fetch('/api/delivery-projects/station.dual_slot_shuttle_press_cell/physical-evidence');
      if (!response.ok) throw new Error(`physical evidence request failed: ${response.status}`);
      return response.json();
    });
    const latestPoint = pointProjectionAfter.point_checks.points[0]?.latest_observation;
    const pointObservation = {
      observationCountBefore: pointProjectionBefore.observations.length,
      observationCountAfter: pointProjectionAfter.observations.length,
      uploadCountBefore: pointProjectionBefore.uploads.length,
      uploadCountAfter: pointProjectionAfter.uploads.length,
      projectedStatus: pointProjectionAfter.point_checks.points[0]?.status,
      latestStatus: latestPoint?.status,
      latestNote: latestPoint?.note,
      latestPhotoUploadId: latestPoint?.photo_upload_id,
      observer: latestPoint?.user?.name,
    };
    if (pointObservation.observationCountAfter !== pointObservation.observationCountBefore + 1
      || pointObservation.uploadCountAfter !== pointObservation.uploadCountBefore + 1
      || pointObservation.projectedStatus !== 'blocked'
      || pointObservation.latestStatus !== 'blocked'
      || pointObservation.latestNote !== pointNote
      || !pointObservation.latestPhotoUploadId
      || !pointObservation.observer) {
      throw new Error(`point observation projection did not refresh: ${JSON.stringify(pointObservation)}`);
    }
    await page.screenshot({ path: path.join(artifactDir, `workbench-point-projection-${viewport.width}x${viewport.height}.png`) });

    await page.evaluate(() => {
      const button = [...document.querySelectorAll('.wb-editor-tab-strip button')]
        .find((item) => item.textContent?.includes('Project Overview'));
      if (!(button instanceof HTMLButtonElement)) throw new Error('overview tab missing');
      button.click();
    });
    await page.waitForSelector('.wb-hold-row');
    const signAction = await page.evaluate(() => {
      const button = document.querySelector('.wb-hold-row .wb-button');
      if (!(button instanceof HTMLButtonElement)) return { found: false };
      button.scrollIntoView({ block: 'center' });
      button.click();
      return { found: true, text: button.textContent, disabled: button.disabled };
    });
    if (!signAction.found || signAction.disabled) throw new Error(`authorized wiring signature action unavailable: ${JSON.stringify(signAction)}`);
    await page.waitForSelector('.wb-sign-dialog');
    const signatureDialog = await page.evaluate(() => ({
      title: document.querySelector('.wb-sign-dialog h2')?.textContent,
      digestRows: document.querySelectorAll('.wb-digest-preview > div').length,
      hasAttestation: Boolean(document.querySelector('.wb-sign-attestation input[type="checkbox"]')),
      overflow: (() => {
        const dialog = document.querySelector('.wb-sign-dialog');
        return dialog ? dialog.scrollWidth > dialog.clientWidth : true;
      })(),
    }));
    await page.screenshot({ path: path.join(artifactDir, `workbench-signature-${viewport.width}x${viewport.height}.png`) });

    await page.click('.wb-sign-dialog button[aria-label="Close signature dialog"]');
    await page.evaluate(() => {
      const button = [...document.querySelectorAll('button')]
        .find((item) => item.textContent?.includes('Run Timeline'));
      if (!(button instanceof HTMLButtonElement)) throw new Error('agent timeline command missing');
      button.click();
    });
    await page.waitForSelector('.wb-timeline-view');
    await new Promise((resolve) => setTimeout(resolve, 1500));
    const agentAudit = await page.evaluate(() => ({
      anomalyRows: document.querySelectorAll('.wb-anomaly:not(.wb-correction)').length,
      correctionRows: document.querySelectorAll('.wb-correction').length,
      longSearchSignals: [...document.querySelectorAll('.wb-anomaly dd')]
        .filter((item) => item.textContent?.includes('Long search or repeated trial-and-error detected')).length,
    }));
    if (agentAudit.anomalyRows !== 10 || agentAudit.correctionRows !== 23) {
      throw new Error(`agent audit records incomplete: ${JSON.stringify(agentAudit)}`);
    }
    await page.screenshot({ path: path.join(artifactDir, `workbench-agent-audit-${viewport.width}x${viewport.height}.png`) });

    await pressControlShortcut(page, 'k');
    await page.waitForSelector('.wb-command-palette');
    await page.type('.wb-command-input input', 'Open Verification Evidence', { delay: 20 });
    const paletteResultCount = await page.$$eval('.wb-command-results [role="option"]', (items) => items.length);
    if (paletteResultCount < 1) throw new Error('command palette returned no verification command');
    await page.screenshot({ path: path.join(artifactDir, `workbench-command-palette-${viewport.width}x${viewport.height}.png`) });
    await page.keyboard.press('Enter');
    await page.waitForSelector('.wb-command-palette', { hidden: true });
    await page.waitForFunction(() => document.querySelector('.wb-view-header h1')?.textContent?.includes('Verification'));

    await pressControlShortcut(page, 'k');
    await page.type('.wb-command-input input', 'stage:codegen status:blocked', { delay: 20 });
    await new Promise((resolve) => setTimeout(resolve, 750));
    const stageFilterState = await page.evaluate(() => ({
      inputValue: document.querySelector('.wb-command-input input')?.value,
      results: [...document.querySelectorAll('.wb-command-results [role="option"]')].map((item) => item.textContent?.trim() ?? ''),
      paletteDiagnostics: [...document.querySelectorAll('.wb-command-palette')].map((palette) => ({
        inputValue: palette.querySelector('.wb-command-input input')?.value,
        resultCount: palette.querySelectorAll('.wb-command-results [role="option"]').length,
        display: getComputedStyle(palette).display,
      })),
    }));
    if (stageFilterState.inputValue !== 'stage:codegen status:blocked') {
      throw new Error(`field:value input state failed: ${JSON.stringify(stageFilterState.inputValue)}`);
    }
    if (stageFilterState.results.length < 1 || !stageFilterState.results.every((item) => item.toLowerCase().includes('codegen'))) {
      throw new Error(`field:value stage filter state failed: ${JSON.stringify({ inputValue: stageFilterState.inputValue, resultCount: stageFilterState.results.length, firstResults: stageFilterState.results.slice(0, 8), paletteDiagnostics: stageFilterState.paletteDiagnostics })}`);
    }
    const stageTokenResults = await page.$$eval('.wb-command-results [role="option"]', (items) => items.map((item) => item.textContent?.trim() ?? ''));
    if (stageTokenResults.length < 1 || !stageTokenResults.every((item) => item.toLowerCase().includes('codegen'))) {
      throw new Error(`field:value stage filter failed: ${JSON.stringify(stageTokenResults)}`);
    }
    await page.screenshot({ path: path.join(artifactDir, `workbench-command-field-filter-${viewport.width}x${viewport.height}.png`) });
    await page.keyboard.press('Escape');
    await page.waitForSelector('.wb-command-palette', { hidden: true });

    await pressControlShortcut(page, 'k');
    await page.type('.wb-command-input input', 'diagnostic:VERIFICATION_WARNING', { delay: 20 });
    await page.waitForFunction(() => {
      const input = document.querySelector('.wb-command-input input');
      const items = document.querySelectorAll('.wb-command-results [role="option"]');
      return input instanceof HTMLInputElement && input.value === 'diagnostic:VERIFICATION_WARNING' && items.length > 0;
    });
    const diagnosticTokenResults = await page.$$eval('.wb-command-results [role="option"]', (items) => items.map((item) => item.textContent?.trim() ?? ''));
    if (diagnosticTokenResults.length < 1) throw new Error('field:value diagnostic filter returned no indexed backend diagnostic');
    await page.keyboard.press('Escape');
    await page.waitForSelector('.wb-command-palette', { hidden: true });
    const paletteFieldFilters = { stageTokenResults, diagnosticTokenResults: diagnosticTokenResults.slice(0, 5) };

    await page.evaluate(() => window.dispatchEvent(new KeyboardEvent('keydown', { key: '\\', ctrlKey: true, bubbles: true })));
    await page.waitForFunction(() => document.querySelectorAll('.wb-editor-groups.is-split .wb-editor-group').length === 2);

    await pressControlShortcut(page, 'k');
    await page.type('.wb-command-input input', 'Open Topology', { delay: 20 });
    await page.keyboard.press('Enter');
    await page.waitForSelector('.wb-editor-group.is-secondary .wb-canvas-surface');
    const topologyMoveButton = await page.$('.wb-editor-group.is-secondary [aria-label^="Move Topology"]');
    if (!topologyMoveButton) throw new Error('keyboard-accessible tab move action missing');
    await topologyMoveButton.click();
    await page.waitForFunction(() => [...document.querySelectorAll('.wb-editor-group.is-primary [role="tab"]')].some((tab) => tab.textContent?.includes('Topology')));

    const dimensionsBefore = await page.evaluate(() => ({
      explorer: document.querySelector('.wb-explorer')?.getBoundingClientRect().width ?? 0,
      inspector: document.querySelector('.wb-inspector')?.getBoundingClientRect().width ?? 0,
      bottom: document.querySelector('.wb-bottom-panel')?.getBoundingClientRect().height ?? 0,
      primary: document.querySelector('.wb-editor-group.is-primary')?.getBoundingClientRect().width ?? 0,
    }));
    await page.focus('[aria-label="Resize Explorer"]');
    await page.keyboard.press('ArrowRight');
    await page.focus('[aria-label="Resize Evidence Inspector"]');
    await page.keyboard.press('ArrowRight');
    await page.focus('[aria-label="Resize bottom panel"]');
    await page.keyboard.press('ArrowUp');
    await page.focus('[aria-label="Resize editor groups"]');
    await page.keyboard.press('ArrowRight');
    const dimensionsAfter = await page.evaluate(() => ({
      explorer: document.querySelector('.wb-explorer')?.getBoundingClientRect().width ?? 0,
      inspector: document.querySelector('.wb-inspector')?.getBoundingClientRect().width ?? 0,
      bottom: document.querySelector('.wb-bottom-panel')?.getBoundingClientRect().height ?? 0,
      primary: document.querySelector('.wb-editor-group.is-primary')?.getBoundingClientRect().width ?? 0,
    }));
    if (dimensionsAfter.explorer <= dimensionsBefore.explorer) throw new Error('Explorer keyboard resize did not increase width');
    if (dimensionsAfter.inspector <= dimensionsBefore.inspector) throw new Error('Inspector keyboard resize did not increase width');
    if (dimensionsAfter.bottom <= dimensionsBefore.bottom) throw new Error('Bottom panel keyboard resize did not increase height');
    if (dimensionsAfter.primary <= dimensionsBefore.primary) throw new Error('Editor split keyboard resize did not increase primary width');

    const explorerSeparator = await page.$('[aria-label="Resize Explorer"]');
    const explorerSeparatorBox = await explorerSeparator?.boundingBox();
    if (!explorerSeparatorBox) throw new Error('Explorer pointer resize handle unavailable');
    const pointerResizeBefore = dimensionsAfter.explorer;
    const separatorCenterX = explorerSeparatorBox.x + explorerSeparatorBox.width / 2;
    const separatorCenterY = explorerSeparatorBox.y + explorerSeparatorBox.height / 2;
    await page.mouse.move(separatorCenterX, separatorCenterY);
    await page.mouse.down();
    await page.mouse.move(separatorCenterX + 24, separatorCenterY, { steps: 4 });
    await page.mouse.up();
    await page.waitForFunction(
      (before) => (document.querySelector('.wb-explorer')?.getBoundingClientRect().width ?? 0) >= before + 12,
      {},
      pointerResizeBefore,
    );
    const pointerResizeAfter = await page.$eval('.wb-explorer', (element) => element.getBoundingClientRect().width);
    if (pointerResizeAfter < pointerResizeBefore + 12) throw new Error('Explorer pointer resize did not increase width');

    await page.click('[data-bottom-panel="problems"]');
    await page.waitForSelector('.wb-panel-filters select[aria-label="Group problems"]');
    const problemPanel = await page.evaluate(() => ({
      groupControl: Boolean(document.querySelector('select[aria-label="Group problems"]')),
      filterControl: Boolean(document.querySelector('select[aria-label="Filter problem severity"]')),
      rows: document.querySelectorAll('.wb-grouped-panel .wb-panel-table > button').length,
    }));
    if (!problemPanel.groupControl || !problemPanel.filterControl || problemPanel.rows < 1) {
      throw new Error(`Problems grouping unavailable: ${JSON.stringify(problemPanel)}`);
    }
    await page.evaluate(() => {
      const row = [...document.querySelectorAll('.wb-grouped-panel .wb-panel-table > button')]
        .find((item) => item.textContent?.includes('ANOM-001'));
      if (!(row instanceof HTMLButtonElement)) throw new Error('artifact-backed ANOM-001 problem missing');
      row.click();
    });
    await page.waitForSelector('[data-artifact-path$="anomalies.json"]');
    await page.waitForSelector('.wb-artifact-view .monaco-editor');
    const problemDeepLink = await page.evaluate(() => ({
      activeTab: [...document.querySelectorAll('.wb-editor-tab.is-active [role="tab"]')].map((tab) => tab.textContent?.trim()),
      project: document.querySelector('.wb-project-switcher button.is-selected')?.textContent?.trim(),
      artifactPath: document.querySelector('.wb-artifact-view')?.getAttribute('data-artifact-path'),
      artifactLine: document.querySelector('.wb-artifact-view')?.getAttribute('data-artifact-line'),
      locationLabel: document.querySelector('.wb-artifact-header > span')?.textContent?.trim(),
    }));
    if (!problemDeepLink.artifactPath?.endsWith('/anomalies.json') || problemDeepLink.artifactLine !== '1' || !problemDeepLink.locationLabel?.includes('Ln 1')) {
      throw new Error(`problem artifact line deep-link failed: ${JSON.stringify(problemDeepLink)}`);
    }
    await page.screenshot({ path: path.join(artifactDir, `workbench-artifact-problem-${viewport.width}x${viewport.height}.png`) });

    await page.click('[data-bottom-panel="tests"]');
    await page.waitForSelector('.wb-panel-filters select[aria-label="Group tests"]');
    const groupedPanels = await page.evaluate(() => ({
      testGroupControl: Boolean(document.querySelector('select[aria-label="Group tests"]')),
      testFilterControl: Boolean(document.querySelector('select[aria-label="Filter test status"]')),
      testRows: document.querySelectorAll('.wb-grouped-panel .wb-panel-table > button').length,
      separators: document.querySelectorAll('[role="separator"][tabindex="0"]').length,
      splitGroups: document.querySelectorAll('.wb-editor-groups.is-split .wb-editor-group').length,
      viewportOverflowX: document.documentElement.scrollWidth > window.innerWidth,
      viewportOverflowY: document.documentElement.scrollHeight > window.innerHeight,
    }));
    if (!groupedPanels.testGroupControl || !groupedPanels.testFilterControl || groupedPanels.testRows < 1) throw new Error(`Tests grouping unavailable: ${JSON.stringify(groupedPanels)}`);
    if (groupedPanels.separators < 4 || groupedPanels.splitGroups !== 2) throw new Error(`Layout interaction surface incomplete: ${JSON.stringify(groupedPanels)}`);
    await page.evaluate(() => {
      const row = [...document.querySelectorAll('.wb-grouped-panel .wb-panel-table > button')]
        .find((item) => item.textContent?.includes('Parser') && item.textContent?.includes('compiler_stages'));
      if (!(row instanceof HTMLButtonElement)) throw new Error('artifact-backed Parser test missing');
      row.click();
    });
    await page.waitForSelector('[data-artifact-path$="compiler-stages.json"]');
    const testDeepLink = await page.evaluate(() => ({
      artifactPath: document.querySelector('.wb-artifact-view')?.getAttribute('data-artifact-path'),
      artifactLine: document.querySelector('.wb-artifact-view')?.getAttribute('data-artifact-line'),
      activeTab: [...document.querySelectorAll('.wb-editor-tab.is-active [role="tab"]')].map((tab) => tab.textContent?.trim()),
    }));
    if (!testDeepLink.artifactPath?.endsWith('/compiler-stages.json') || testDeepLink.artifactLine !== '1') {
      throw new Error(`test artifact deep-link failed: ${JSON.stringify(testDeepLink)}`);
    }
    await page.screenshot({ path: path.join(artifactDir, `workbench-interactions-${viewport.width}x${viewport.height}.png`) });

    results.push({ viewport, projectCoverage, overview, wiringRows, pointObservation, signatureDialog, agentAudit, paletteResultCount, paletteFieldFilters, dimensionsBefore, dimensionsAfter, pointerResizeBefore, pointerResizeAfter, problemPanel, problemDeepLink, testDeepLink, groupedPanels });
    await page.close();
  }
} finally {
  await browser.close();
}

const failed = results.some((result) => (
  result.projectCoverage.length !== 3
  || result.projectCoverage.some((project) => !project.heading || !project.selected)
  || result.overview.pipelineRows < 10
  || result.overview.holdRows !== 5
  || !['blocked', 'human_action_required', 'release_approved'].includes(result.overview.releaseStatus)
  || result.overview.viewportOverflowX
  || result.overview.viewportOverflowY
  || result.wiringRows !== 16
  || result.pointObservation.observationCountAfter !== result.pointObservation.observationCountBefore + 1
  || result.pointObservation.uploadCountAfter !== result.pointObservation.uploadCountBefore + 1
  || result.pointObservation.projectedStatus !== 'blocked'
  || result.pointObservation.latestStatus !== 'blocked'
  || !result.pointObservation.latestPhotoUploadId
  || result.signatureDialog.digestRows < 1
  || !result.signatureDialog.hasAttestation
  || result.signatureDialog.overflow
  || result.agentAudit.anomalyRows !== 10
  || result.agentAudit.correctionRows !== 23
  || result.agentAudit.longSearchSignals < 1
  || result.paletteResultCount < 1
  || result.paletteFieldFilters.stageTokenResults.length < 1
  || result.paletteFieldFilters.diagnosticTokenResults.length < 1
  || result.dimensionsAfter.explorer <= result.dimensionsBefore.explorer
  || result.dimensionsAfter.inspector <= result.dimensionsBefore.inspector
  || result.dimensionsAfter.bottom <= result.dimensionsBefore.bottom
  || result.dimensionsAfter.primary <= result.dimensionsBefore.primary
  || result.pointerResizeAfter < result.pointerResizeBefore + 12
  || !result.problemPanel.groupControl
  || !result.problemPanel.filterControl
  || result.problemPanel.rows < 1
  || !result.problemDeepLink.artifactPath?.endsWith('/anomalies.json')
  || result.problemDeepLink.artifactLine !== '1'
  || !result.testDeepLink.artifactPath?.endsWith('/compiler-stages.json')
  || result.testDeepLink.artifactLine !== '1'
  || result.groupedPanels.testRows < 1
  || !result.groupedPanels.testGroupControl
  || !result.groupedPanels.testFilterControl
  || result.groupedPanels.separators < 4
  || result.groupedPanels.splitGroups !== 2
  || result.groupedPanels.viewportOverflowX
  || result.groupedPanels.viewportOverflowY
));
console.log(JSON.stringify({ ok: !failed, results }, null, 2));
if (failed) process.exitCode = 1;
