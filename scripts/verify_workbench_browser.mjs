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

async function replaceInputValue(page, selector, value) {
  await page.focus(selector);
  await pressControlShortcut(page, 'a');
  await page.keyboard.type(value, { delay: 12 });
}

async function setControlledValue(page, selector, value) {
  await page.$eval(selector, (element, nextValue) => {
    const prototype = element instanceof HTMLTextAreaElement
      ? HTMLTextAreaElement.prototype
      : HTMLInputElement.prototype;
    const setter = Object.getOwnPropertyDescriptor(prototype, 'value')?.set;
    if (!setter) throw new Error(`value setter unavailable for ${element.tagName}`);
    setter.call(element, nextValue);
    element.dispatchEvent(new Event('input', { bubbles: true }));
    element.dispatchEvent(new Event('change', { bubbles: true }));
  }, value);
}

async function setPaletteQuery(page, query) {
  await page.evaluate(() => {
    const palettes = [...document.querySelectorAll('.wb-command-palette')]
      .filter((item) => item.getClientRects().length > 0);
    const input = palettes.at(-1)?.querySelector('.wb-command-input input');
    if (!(input instanceof HTMLInputElement)) throw new Error('visible command palette input missing');
    input.focus();
  });
  await pressControlShortcut(page, 'a');
  await page.keyboard.type(query, { delay: 12 });
  await page.waitForFunction((expected) => {
    const palettes = [...document.querySelectorAll('.wb-command-palette')]
      .filter((item) => item.getClientRects().length > 0);
    const active = palettes.at(-1);
    const results = active?.querySelector('.wb-command-results');
    const input = active?.querySelector('.wb-command-input input');
    return input instanceof HTMLInputElement
      && input.value === expected
      && results?.getAttribute('data-query') === expected;
  }, {}, query);
}

async function paletteResultState(page) {
  return page.evaluate(() => ({
    ...(() => {
      const palettes = [...document.querySelectorAll('.wb-command-palette')]
        .filter((item) => item.getClientRects().length > 0);
      const results = palettes.at(-1)?.querySelector('.wb-command-results');
      return {
        visiblePaletteCount: palettes.length,
        query: results?.getAttribute('data-query'),
        filteredCount: Number(results?.getAttribute('data-filtered-count') ?? -1),
        items: [...(results?.querySelectorAll('[role="option"]') ?? [])].map((item) => ({
      id: item.getAttribute('data-command-id'),
      text: item.textContent?.trim() ?? '',
      project: item.getAttribute('data-search-project'),
      stage: item.getAttribute('data-search-stage'),
      diagnostic: item.getAttribute('data-search-diagnostic'),
      evidence: item.getAttribute('data-search-evidence'),
      commit: item.getAttribute('data-search-commit'),
      status: item.getAttribute('data-search-status'),
        })),
      };
    })(),
  }));
}

async function selectProject(page, projectId) {
  await page.evaluate((id) => {
    const button = document.querySelector(`.wb-project-switcher button[data-project-id="${id}"]`);
    if (!(button instanceof HTMLButtonElement)) throw new Error(`project missing: ${id}`);
    button.click();
  }, projectId);
  await page.waitForFunction((id) => document.querySelector('.wb-project-switcher button.is-selected')?.getAttribute('data-project-id') === id, {}, projectId);
  await page.evaluate(() => {
    const overview = [...document.querySelectorAll('.wb-tree-leaf')]
      .find((item) => item.textContent?.includes('Project Overview'));
    if (overview instanceof HTMLButtonElement) overview.click();
  });
  await page.waitForFunction((id) => document.querySelector('.wb-view-header p')?.textContent?.includes(id), {}, projectId);
  return page.evaluate(() => ({
    selected: document.querySelector('.wb-project-switcher button.is-selected')?.textContent?.trim(),
    heading: document.querySelector('.wb-view-header h1')?.textContent?.trim(),
  }));
}

async function auditProjectSurface(page, projectId) {
  const selection = await selectProject(page, projectId);
  await page.waitForFunction(() => document.querySelectorAll('.wb-pipeline-row').length >= 10);
  await page.waitForFunction(() => document.querySelectorAll('.wb-hold-row').length === 5);
  const apiEvidence = await page.evaluate(async (id) => {
    const [projectResponse, wiringResponse, physicalResponse, releaseResponse] = await Promise.all([
      fetch(`/api/delivery-projects/${id}`),
      fetch(`/api/delivery-projects/${id}/wiring`),
      fetch(`/api/delivery-projects/${id}/physical-evidence`),
      fetch(`/api/delivery-projects/${id}/release`),
    ]);
    if (![projectResponse, wiringResponse, physicalResponse, releaseResponse].every((response) => response.ok)) {
      throw new Error(`project evidence request failed for ${id}`);
    }
    const [project, wiring, physical, release] = await Promise.all([
      projectResponse.json(),
      wiringResponse.json(),
      physicalResponse.json(),
      releaseResponse.json(),
    ]);
    const hilReview = release.holds?.find((hold) => hold.hold_id === 'hil_review');
    const hilLabel = hilReview?.status === 'human_confirmed'
      ? 'confirmed'
      : hilReview?.status === 'human_action_required'
        ? 'action required'
        : hilReview?.status;
    const hilStatusText = [...document.querySelectorAll('.wb-statusbar > span')]
      .find((item) => item.textContent?.trim().startsWith('HIL '))?.textContent?.trim() ?? '';
    return {
      pipelineRows: document.querySelectorAll('.wb-pipeline-row').length,
      holdRows: document.querySelectorAll('.wb-hold-row').length,
      releaseStatus: document.querySelector('[data-release-status]')?.getAttribute('data-release-status'),
      wiringPointCount: wiring.points?.length ?? 0,
      physicalPointCount: physical.point_checks?.points?.length ?? 0,
      normalizedWiring: (wiring.points ?? []).every((point) => point.controller && point.channel && point.compiler_status),
      executionVerdict: project.latest_run?.attribution?.execution_unattended_verdict,
      sourceAuthoringVerdict: project.latest_run?.attribution?.source_authoring_verdict,
      unattendedVerdict: project.latest_run?.attribution?.unattended_verdict,
      hilReviewStatus: hilReview?.status,
      hilReviewReason: hilReview?.reason,
      hilStatusBound: Boolean(hilLabel && hilStatusText === `HIL ${hilLabel}`),
      inspectorHilReasonVisible: Boolean(hilReview?.reason && document.querySelector('.wb-release-projection')?.textContent?.includes(hilReview.reason)),
      inspectorBlockedPrerequisites: document.querySelectorAll('.wb-prerequisite-list li').length,
    };
  }, projectId);
  await page.evaluate(() => {
    const button = [...document.querySelectorAll('button')]
      .find((item) => item.textContent?.includes('Controller I/O and Point Checks'));
    if (!(button instanceof HTMLButtonElement)) throw new Error('wiring view command missing');
    button.click();
  });
  await page.waitForFunction((count) => document.querySelectorAll('.wb-wiring-table tbody tr').length === count, {}, apiEvidence.wiringPointCount);
  const wiringSurface = await page.evaluate(() => {
    const rows = [...document.querySelectorAll('.wb-wiring-table tbody tr')];
    return {
      rows: rows.length,
      unknownControllers: rows.filter((row) => row.querySelector('td:first-child strong')?.textContent?.trim() === 'Unknown').length,
      missingCompiler: rows.filter((row) => [...row.querySelectorAll('td')].some((cell) => cell.textContent?.trim() === 'Missing')).length,
      inputSafeStateMismatch: rows.filter((row) => {
        const cells = row.querySelectorAll('td');
        return cells[2]?.textContent?.trim() === 'input' && cells[5]?.textContent?.trim() !== 'n/a';
      }).length,
    };
  });
  await page.evaluate(() => {
    const overview = [...document.querySelectorAll('.wb-tree-leaf')]
      .find((item) => item.textContent?.includes('Project Overview'));
    if (!(overview instanceof HTMLButtonElement)) throw new Error('project overview command missing');
    overview.click();
  });
  await page.waitForFunction((id) => document.querySelector('.wb-view-header p')?.textContent?.includes(id), {}, projectId);
  return { projectId, ...selection, ...apiEvidence, wiringSurface };
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
    const requiredProjects = ['line.three_station_assembly', 'module.axis_move_blocking_baseline', 'station.dual_slot_shuttle_press_cell'];
    const projectIds = await page.$$eval('.wb-project-switcher button', (buttons) => buttons.map((button) => button.getAttribute('data-project-id')));
    const missingProjects = requiredProjects.filter((id) => !projectIds.includes(id));
    if (missingProjects.length > 0) throw new Error(`required delivery projects missing: ${missingProjects.join(', ')}`);
    const projectCoverage = [];
    for (const projectName of requiredProjects) projectCoverage.push(await auditProjectSurface(page, projectName));
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
      responsibilitySteps: document.querySelectorAll('.wb-responsibility-step').length,
      responsibilityStages: [...document.querySelectorAll('.wb-responsibility-step')]
        .map((item) => item.getAttribute('data-responsibility-stage')),
      agentAuthoringVerdict: document.querySelector('[data-responsibility-stage="agent-authoring"]')?.getAttribute('data-verdict'),
      humanOwnedStages: document.querySelectorAll('[data-responsibility-owner="human"]').length,
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
      const button = [...document.querySelectorAll('.wb-tree-leaf')]
        .find((item) => item.textContent?.trim() === 'Topology');
      if (!(button instanceof HTMLButtonElement)) throw new Error('delivery project Topology command missing');
      button.click();
    });
    await page.waitForFunction(() => document.querySelector('.wb-geometry-surface')?.textContent?.includes('Semantic Twin Geometry'));
    const topology = await page.evaluate(async () => {
      const response = await fetch('/api/delivery-projects/station.dual_slot_shuttle_press_cell/geometry');
      if (!response.ok) throw new Error(`delivery geometry request failed: ${response.status}`);
      const artifact = await response.json();
      const surface = document.querySelector('.wb-geometry-surface');
      if (artifact.status === 'missing') {
        const blockerCode = artifact.blocker?.code;
        return {
          mode: 'missing',
          blockerCode,
          blockerVisible: Boolean(blockerCode && surface?.textContent?.includes(blockerCode)),
          nodeEvidenceRecords: 0,
          edgeEvidenceRecords: 0,
          renderedNodes: 0,
          renderedEdges: 0,
        };
      }
      const nodeEvidenceRecords = (artifact.nodes ?? []).filter((node) => node.evidence_status).length;
      const edgeEvidenceRecords = (artifact.edges ?? []).filter((edge) => edge.evidence_status).length;
      const graph = surface?.querySelector('svg[aria-label^="Semantic twin geometry"]');
      return {
        mode: 'rendered',
        blockerCode: null,
        blockerVisible: false,
        nodeEvidenceRecords,
        edgeEvidenceRecords,
        renderedNodes: graph?.querySelectorAll('circle').length ?? 0,
        renderedEdges: graph?.querySelectorAll('path[stroke]').length ?? 0,
      };
    });
    if (topology.mode === 'missing') {
      if (topology.blockerCode !== 'DELIVERY_GEOMETRY_ARTIFACT_MISSING' || !topology.blockerVisible) {
        throw new Error(`delivery geometry missing state is not explicit: ${JSON.stringify(topology)}`);
      }
    } else if ((topology.nodeEvidenceRecords < 1 && topology.edgeEvidenceRecords < 1)
      || (topology.renderedNodes < 1 && topology.renderedEdges < 1)) {
      throw new Error(`delivery geometry evidence did not render: ${JSON.stringify(topology)}`);
    }
    let topologyKeyboard = { mode: topology.mode };
    if (topology.mode === 'rendered') {
      const nodeSelector = '.wb-geometry-surface .geometry-reference-item--node[role="button"][tabindex="0"]';
      const edgeSelector = '.wb-geometry-surface .geometry-reference-item--edge[role="button"][tabindex="0"]';
      await page.waitForSelector(nodeSelector);
      await page.waitForSelector(edgeSelector);
      await page.$eval(nodeSelector, (item) => item.focus());
      const nodeLabel = await page.$eval(nodeSelector, (item) => item.getAttribute('aria-label'));
      await page.keyboard.press('Enter');
      await page.waitForSelector('.geometry-reference-detail[data-reference-kind="node"]');
      const nodeDetail = await page.$eval('.geometry-reference-detail[data-reference-kind="node"]', (item) => ({
        text: item.textContent?.trim() ?? '',
        evidence: item.getAttribute('data-evidence-status'),
      }));
      await page.$eval(edgeSelector, (item) => item.focus());
      const edgeLabel = await page.$eval(edgeSelector, (item) => item.getAttribute('aria-label'));
      await page.keyboard.press('Space');
      await page.waitForSelector('.geometry-reference-detail[data-reference-kind="edge"]');
      const edgeDetail = await page.$eval('.geometry-reference-detail[data-reference-kind="edge"]', (item) => ({
        text: item.textContent?.trim() ?? '',
        evidence: item.getAttribute('data-evidence-status'),
      }));
      if (!nodeLabel?.includes('evidence status') || !nodeDetail.text.includes('Evidence status:') || !nodeDetail.evidence
        || !edgeLabel?.includes('evidence status') || !edgeDetail.text.includes('Evidence status:') || !edgeDetail.evidence) {
        throw new Error(`geometry keyboard/status semantics incomplete: ${JSON.stringify({ nodeLabel, nodeDetail, edgeLabel, edgeDetail })}`);
      }
      topologyKeyboard = { mode: 'rendered', nodeLabel, nodeDetail, edgeLabel, edgeDetail };
    }
    await page.screenshot({ path: path.join(artifactDir, `workbench-topology-${viewport.width}x${viewport.height}.png`) });

    await page.evaluate(() => {
      const button = [...document.querySelectorAll('button')]
        .find((item) => item.textContent?.includes('Controller I/O and Point Checks'));
      if (!(button instanceof HTMLButtonElement)) throw new Error('wiring view command missing');
      button.click();
    });
    await page.waitForFunction(() => document.querySelectorAll('.wb-data-table tbody tr').length === 16);
    const wiringRows = await page.$$eval('.wb-data-table tbody tr', (rows) => rows.length);
    const wiringDiagnostics = await page.$$eval('.wb-wiring-diagnostic', (rows) => rows.length);
    if (wiringDiagnostics !== 0) throw new Error(`canonical delivery wiring has ${wiringDiagnostics} validation diagnostics`);
    await page.screenshot({ path: path.join(artifactDir, `workbench-wiring-${viewport.width}x${viewport.height}.png`) });

    const pointProjectionBefore = await page.evaluate(async () => {
      const response = await fetch('/api/delivery-projects/station.dual_slot_shuttle_press_cell/physical-evidence');
      if (!response.ok) throw new Error(`physical evidence request failed: ${response.status}`);
      return response.json();
    });
    const firstPointAction = await page.$('.wb-wiring-table tbody tr .wb-icon-command');
    if (!firstPointAction) throw new Error('point observation action missing');
    await firstPointAction.click();
    await page.waitForSelector('.wb-point-dialog', { visible: true });
    await page.waitForFunction(() => document.activeElement?.getAttribute('aria-label') === 'Close point observation dialog');
    await page.keyboard.press('Escape');
    await page.waitForSelector('.wb-point-dialog', { hidden: true });
    const pointDialogFocusRestored = await page.evaluate(() => document.activeElement?.classList.contains('wb-icon-command'));
    if (!pointDialogFocusRestored) throw new Error('point observation dialog did not restore trigger focus after Escape');
    const reopenedPointAction = await page.$('.wb-wiring-table tbody tr .wb-icon-command');
    if (!reopenedPointAction) throw new Error('point observation action missing after focus restoration');
    await reopenedPointAction.click();
    await page.waitForSelector('.wb-point-dialog', { visible: true });
    await page.waitForSelector('.wb-point-dialog .wb-photo-input input[type="file"]');
    await page.evaluate(() => {
      const button = [...document.querySelectorAll('.wb-point-dialog [aria-label="Point observation status"] button')]
        .find((item) => item.textContent?.trim() === 'blocked');
      if (!(button instanceof HTMLButtonElement)) throw new Error('blocked point status control missing');
      button.click();
    });
    await page.waitForFunction(() => [...document.querySelectorAll('.wb-point-dialog [aria-label="Point observation status"] button')]
      .some((item) => item.textContent?.trim() === 'blocked' && item.getAttribute('aria-pressed') === 'true'));
    const pointNote = `Automated browser validation ${viewport.width}x${viewport.height}; not physical acceptance.`;
    await setControlledValue(page, '.wb-point-dialog textarea', pointNote);
    await page.waitForFunction((expected) => document.querySelector('.wb-point-dialog textarea')?.value === expected, {}, pointNote);
    const photoInput = await page.$('.wb-photo-input input[type="file"]');
    if (!photoInput) throw new Error('point photo input missing');
    const pointPhotoName = `workbench-overview-${viewport.width}x${viewport.height}.png`;
    await photoInput.uploadFile(path.join(artifactDir, pointPhotoName));
    await page.waitForFunction((expected) => document.querySelector('.wb-photo-input > span:last-of-type')?.textContent?.trim() === expected, {}, pointPhotoName);
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
      return { found: true, text: button.textContent, disabled: button.disabled };
    });
    if (!signAction.found || signAction.disabled) throw new Error(`authorized wiring signature action unavailable: ${JSON.stringify(signAction)}`);
    await page.click('.wb-hold-row .wb-button');
    await page.waitForSelector('.wb-sign-dialog');
    await page.waitForFunction(() => document.activeElement?.getAttribute('aria-label') === 'Close signature dialog');
    await page.keyboard.down('Shift');
    await page.keyboard.press('Tab');
    await page.keyboard.up('Shift');
    const signatureFocusTrapped = await page.evaluate(() => Boolean(document.activeElement?.closest('.wb-sign-dialog')));
    if (!signatureFocusTrapped) throw new Error('signature dialog did not retain keyboard focus');
    await page.keyboard.press('Escape');
    await page.waitForSelector('.wb-sign-dialog', { hidden: true });
    const signatureFocusRestored = await page.evaluate(() => Boolean(document.activeElement?.closest('.wb-hold-row')));
    if (!signatureFocusRestored) throw new Error('signature dialog did not restore trigger focus after Escape');
    await page.click('.wb-hold-row .wb-button');
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
    const agentAudit = await page.evaluate(async () => {
      const projectId = document.querySelector('.wb-project-switcher button.is-selected')?.getAttribute('data-project-id');
      const runId = document.querySelector('.wb-timeline-view h1')?.textContent?.replace(/^Agent Run\s+/, '').trim();
      if (!projectId || !runId) throw new Error('selected project/run identity missing from agent timeline');
      const response = await fetch(`/api/delivery-projects/${projectId}/runs/${runId}`);
      if (!response.ok) throw new Error(`agent run request failed: ${response.status}`);
      const run = await response.json();
      const sourceAnomalies = run.anomalies ?? run.documents?.anomalies?.records ?? [];
      const sourceCorrections = run.corrections ?? run.documents?.corrections?.records ?? [];
      return {
        anomalyRows: document.querySelectorAll('.wb-anomaly:not(.wb-correction)').length,
        correctionRows: document.querySelectorAll('.wb-correction').length,
        expectedAnomalies: sourceAnomalies.length,
        expectedCorrections: sourceCorrections.length,
        sourceAuthoringScope: document.querySelector('[data-attribution-scope="source-authoring"]')?.textContent?.trim() ?? '',
        sourceAuthoringVerdict: document.querySelector('[data-attribution-scope="source-authoring"]')?.getAttribute('data-verdict') ?? '',
        materializationScope: document.querySelector('[data-attribution-scope="materialization-execution"]')?.textContent?.trim() ?? '',
        materializationVerdict: document.querySelector('[data-attribution-scope="materialization-execution"]')?.getAttribute('data-verdict') ?? '',
        longSearchSignals: [...document.querySelectorAll('.wb-anomaly dd')]
          .filter((item) => item.textContent?.includes('Long search or repeated trial-and-error detected')).length,
      };
    });
    if (agentAudit.anomalyRows !== agentAudit.expectedAnomalies || agentAudit.correctionRows !== agentAudit.expectedCorrections) {
      throw new Error(`agent audit records incomplete: ${JSON.stringify(agentAudit)}`);
    }
    if (!agentAudit.sourceAuthoringScope.includes('Source authoring') || !/not[_ ]proven/i.test(agentAudit.sourceAuthoringVerdict)) {
      throw new Error(`agent run source authoring boundary is not explicit: ${JSON.stringify(agentAudit)}`);
    }
    if (!agentAudit.materializationScope.includes('Materialization execution') || agentAudit.materializationVerdict !== 'proven') {
      throw new Error(`agent run materialization execution proof is not explicit: ${JSON.stringify(agentAudit)}`);
    }
    await page.screenshot({ path: path.join(artifactDir, `workbench-agent-audit-${viewport.width}x${viewport.height}.png`) });

    await pressControlShortcut(page, 'k');
    await page.waitForSelector('.wb-command-palette');
    await setPaletteQuery(page, 'Open Verification Evidence');
    const initialPaletteState = await paletteResultState(page);
    const paletteResultCount = initialPaletteState.items.length;
    if (initialPaletteState.visiblePaletteCount !== 1) throw new Error(`expected one visible command palette: ${JSON.stringify(initialPaletteState)}`);
    if (paletteResultCount < 1) throw new Error('command palette returned no verification command');
    await page.screenshot({ path: path.join(artifactDir, `workbench-command-palette-${viewport.width}x${viewport.height}.png`) });
    await page.keyboard.press('Enter');
    await page.waitForSelector('.wb-command-palette', { hidden: true });
    await page.waitForFunction(() => document.querySelector('.wb-view-header h1')?.textContent?.includes('Verification'));

    await page.evaluate(() => {
      const button = [...document.querySelectorAll('.wb-segmented button')]
        .find((item) => item.textContent?.trim() === 'blocked');
      if (!(button instanceof HTMLButtonElement)) throw new Error('blocked evidence filter missing');
      button.click();
    });
    await page.waitForFunction(() => document.querySelector('.wb-segmented button[aria-pressed="true"]')?.textContent?.trim() === 'blocked');
    const evidenceFilterStored = await page.evaluate(() => {
      const persisted = JSON.parse(localStorage.getItem('rustplc-workbench-layout') ?? '{}');
      return persisted.state?.evidenceFilter;
    });
    if (evidenceFilterStored !== 'blocked') throw new Error(`evidence filter was not persisted: ${evidenceFilterStored}`);
    await page.evaluate(() => {
      const button = [...document.querySelectorAll('.wb-project-switcher button')]
        .find((item) => !item.classList.contains('is-selected'));
      if (!(button instanceof HTMLButtonElement)) throw new Error('alternate project missing');
      button.click();
    });
    await page.waitForFunction(() => !document.querySelector('.wb-project-switcher button.is-selected')?.textContent?.includes('Dual-slot Shuttle Press Cell'));
    await page.evaluate(() => {
      const button = [...document.querySelectorAll('.wb-project-switcher button')]
        .find((item) => item.textContent?.includes('Dual-slot Shuttle Press Cell'));
      if (!(button instanceof HTMLButtonElement)) throw new Error('canonical project missing after filter switch');
      button.click();
    });
    await page.waitForFunction(() => document.querySelector('.wb-project-switcher button.is-selected')?.textContent?.includes('Dual-slot Shuttle Press Cell'));
    await page.waitForFunction(() => document.querySelector('.wb-segmented button[aria-pressed="true"]')?.textContent?.trim() === 'blocked');
    await page.waitForFunction(() => [...document.querySelectorAll('.wb-stage-row')]
      .some((row) => row.textContent?.toLowerCase().includes('codegen') && row.textContent?.toLowerCase().includes('blocked')));
    const evidenceFilterPersistence = { stored: evidenceFilterStored, restored: true };

    const searchFixture = await page.evaluate(async () => {
      const projectId = 'station.dual_slot_shuttle_press_cell';
      const [projectResponse, evidenceResponse] = await Promise.all([
        fetch(`/api/delivery-projects/${projectId}`),
        fetch(`/api/delivery-projects/${projectId}/evidence`),
      ]);
      if (!projectResponse.ok || !evidenceResponse.ok) throw new Error('search fixture API unavailable');
      const project = await projectResponse.json();
      const evidencePayload = await evidenceResponse.json();
      const evidenceItems = Array.isArray(evidencePayload)
        ? evidencePayload
        : evidencePayload.evidence ?? evidencePayload.items ?? [];
      const evidence = evidenceItems.find((item) => item.evidence_state && (item.source_commit || project.source_commit));
      if (!project.source_commit || !evidence) throw new Error('search fixture lacks commit-bound evidence');
      return {
        projectId,
        projectCommit: project.source_commit,
        evidenceState: evidence.evidence_state,
        evidenceCommit: evidence.source_commit ?? project.source_commit,
      };
    });

    await pressControlShortcut(page, 'k');
    await setPaletteQuery(page, 'stage:codegen status:blocked');
    const stageFilterState = await paletteResultState(page);
    if (stageFilterState.visiblePaletteCount !== 1 || stageFilterState.filteredCount < 1 || stageFilterState.items.length < 1
      || !stageFilterState.items.every((item) => item.stage?.includes('codegen') && item.status?.includes('blocked'))) {
      throw new Error(`field:value stage filter failed: ${JSON.stringify(stageFilterState)}`);
    }
    const stageTokenResults = stageFilterState.items.map((item) => item.text);
    await page.screenshot({ path: path.join(artifactDir, `workbench-command-field-filter-${viewport.width}x${viewport.height}.png`) });
    await page.focus('.wb-command-input input');
    await page.keyboard.press('Escape');
    await page.waitForSelector('.wb-command-palette', { hidden: true });

    await pressControlShortcut(page, 'k');
    await setPaletteQuery(page, 'diagnostic:VERIFICATION_WARNING');
    const diagnosticFilterState = await paletteResultState(page);
    if (diagnosticFilterState.visiblePaletteCount !== 1 || diagnosticFilterState.filteredCount < 1 || diagnosticFilterState.items.length < 1
      || !diagnosticFilterState.items.every((item) => item.diagnostic?.includes('verification_warning'))) {
      throw new Error(`field:value diagnostic filter failed: ${JSON.stringify(diagnosticFilterState)}`);
    }
    const diagnosticTokenResults = diagnosticFilterState.items.map((item) => item.text);
    await page.keyboard.press('Escape');
    await page.waitForSelector('.wb-command-palette', { hidden: true });

    await pressControlShortcut(page, 'k');
    const evidenceQuery = `category:evidence evidence:${searchFixture.evidenceState} commit:${searchFixture.evidenceCommit.slice(0, 10)}`;
    await setPaletteQuery(page, evidenceQuery);
    const evidenceFilterState = await paletteResultState(page);
    if (evidenceFilterState.visiblePaletteCount !== 1 || evidenceFilterState.filteredCount < 1 || evidenceFilterState.items.length < 1
      || !evidenceFilterState.items.every((item) => item.id?.startsWith('evidence-')
        && item.evidence?.includes(searchFixture.evidenceState.toLowerCase())
        && item.commit?.includes(searchFixture.evidenceCommit.slice(0, 10).toLowerCase()))) {
      throw new Error(`field:value evidence/commit filter failed: ${JSON.stringify(evidenceFilterState)}`);
    }
    await page.keyboard.press('Escape');
    await page.waitForSelector('.wb-command-palette', { hidden: true });

    await page.click('.wb-activity-bar button[aria-label="Search"]');
    const projectQuery = `category:project project:${searchFixture.projectId} commit:${searchFixture.projectCommit.slice(0, 10)}`;
    await replaceInputValue(page, '.wb-search-explorer input', projectQuery);
    await page.keyboard.press('Enter');
    await page.waitForSelector('.wb-command-palette');
    const projectFilterState = await paletteResultState(page);
    if (projectFilterState.visiblePaletteCount !== 1 || projectFilterState.query !== projectQuery || projectFilterState.filteredCount !== 1
      || projectFilterState.items.length !== 1 || !projectFilterState.items[0].id?.startsWith('project-')
      || !projectFilterState.items[0].project?.includes(searchFixture.projectId)
      || !projectFilterState.items[0].commit?.includes(searchFixture.projectCommit.slice(0, 10).toLowerCase())) {
      throw new Error(`Search explorer project/commit filter failed: ${JSON.stringify(projectFilterState)}`);
    }
    await page.focus('.wb-command-input input');
    await page.keyboard.press('Escape');
    await page.waitForSelector('.wb-command-palette', { hidden: true });

    await page.click('.wb-command-center');
    await page.waitForSelector('.wb-command-palette');
    await page.waitForFunction(() => document.activeElement?.matches('.wb-command-input input'));
    await page.keyboard.press('Escape');
    await page.waitForSelector('.wb-command-palette', { hidden: true });
    const paletteFocusRestored = await page.evaluate(() => document.activeElement?.classList.contains('wb-command-center'));
    if (!paletteFocusRestored) throw new Error('command palette did not restore command-center focus');
    const paletteFieldFilters = {
      stageTokenResults,
      diagnosticTokenResults: diagnosticTokenResults.slice(0, 5),
      evidenceResults: evidenceFilterState.items.map((item) => item.text),
      projectResults: projectFilterState.items.map((item) => item.text),
      paletteFocusRestored,
    };

    await page.evaluate(() => window.dispatchEvent(new KeyboardEvent('keydown', { key: '\\', ctrlKey: true, bubbles: true })));
    await page.waitForFunction(() => document.querySelectorAll('.wb-editor-groups.is-split .wb-editor-group').length === 2);
    const splitSurface = await page.evaluate(() => ({
      groups: document.querySelectorAll('.wb-editor-groups.is-split .wb-editor-group').length,
      separators: document.querySelectorAll('[role="separator"][tabindex="0"]').length,
    }));

    await pressControlShortcut(page, 'k');
    await page.type('.wb-command-input input', 'Open Topology', { delay: 20 });
    await page.keyboard.press('Enter');
    await page.waitForFunction(() => [...document.querySelectorAll('.wb-editor-group')]
      .some((group) => group.querySelector('.wb-geometry-surface')
        && [...group.querySelectorAll('[role="tab"][aria-selected="true"]')].some((tab) => tab.textContent?.includes('Topology'))));
    const topologySourceGroup = await page.evaluate(() => document.querySelector('.wb-editor-group.is-secondary .wb-geometry-surface') ? 'secondary' : 'primary');
    const topologyTargetGroup = topologySourceGroup === 'secondary' ? 'primary' : 'secondary';
    const topologyMoveButton = await page.$(`.wb-editor-group.is-${topologySourceGroup} [aria-label^="Move Topology"]`);
    if (!topologyMoveButton) throw new Error('keyboard-accessible tab move action missing');
    await topologyMoveButton.click();
    await page.waitForFunction((targetGroup) => [...document.querySelectorAll(`.wb-editor-group.is-${targetGroup} [role="tab"]`)].some((tab) => tab.textContent?.includes('Topology')), {}, topologyTargetGroup);
    await page.waitForSelector(`.wb-editor-group.is-${topologyTargetGroup} .wb-geometry-surface`);

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
      groupOptions: [...document.querySelectorAll('select[aria-label="Group problems"] option')].map((option) => option.value),
    }));
    if (!problemPanel.groupControl || !problemPanel.filterControl || problemPanel.rows < 1 || !problemPanel.groupOptions.includes('commit')) {
      throw new Error(`Problems grouping unavailable: ${JSON.stringify(problemPanel)}`);
    }
    await page.select('select[aria-label="Group problems"]', 'commit');
    await page.waitForFunction(() => [...document.querySelectorAll('.wb-grouped-panel section > h3')]
      .some((heading) => /[0-9a-f]{7,40}/i.test(heading.textContent ?? '')));
    const problemCommitGroups = await page.$$eval('.wb-grouped-panel section > h3', (headings) => headings.map((heading) => heading.textContent?.trim() ?? ''));
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
      testGroupValue: document.querySelector('select[aria-label="Group tests"]')?.value,
      testGroupOptions: [...document.querySelectorAll('select[aria-label="Group tests"] option')].map((option) => option.value),
      testSourceGroups: [...document.querySelectorAll('.wb-grouped-panel section:not(.wb-test-sources) > h3')].map((heading) => heading.textContent?.trim() ?? ''),
      separators: document.querySelectorAll('[role="separator"][tabindex="0"]').length,
      splitGroups: document.querySelectorAll('.wb-editor-groups.is-split .wb-editor-group').length,
      viewportOverflowX: document.documentElement.scrollWidth > window.innerWidth,
      viewportOverflowY: document.documentElement.scrollHeight > window.innerHeight,
    }));
    if (!groupedPanels.testGroupControl || !groupedPanels.testFilterControl || groupedPanels.testRows < 1
      || groupedPanels.testGroupValue !== 'source' || !groupedPanels.testGroupOptions.includes('source')
      || !groupedPanels.testSourceGroups.every((group) => /Library|Integration|Canonical example|Delivery project|Unclassified source/i.test(group))
      || !['Library', 'Integration', 'Canonical example', 'Delivery project'].every((expected) => groupedPanels.testSourceGroups.some((group) => group.startsWith(expected)))) {
      throw new Error(`Tests grouping unavailable: ${JSON.stringify(groupedPanels)}`);
    }
    if (splitSurface.groups !== 2 || splitSurface.separators < 4 || groupedPanels.separators < 3) throw new Error(`Layout interaction surface incomplete: ${JSON.stringify({ splitSurface, groupedPanels })}`);
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

    results.push({ viewport, projectCoverage, overview, topology, topologyKeyboard, wiringRows, wiringDiagnostics, pointObservation, signatureDialog, agentAudit, paletteResultCount, paletteFieldFilters, evidenceFilterPersistence, splitSurface, dimensionsBefore, dimensionsAfter, pointerResizeBefore, pointerResizeAfter, problemPanel, problemCommitGroups, problemDeepLink, testDeepLink, groupedPanels });
    await page.close();
  }
} finally {
  await browser.close();
}

const failed = results.some((result) => (
  result.projectCoverage.length !== 3
  || result.projectCoverage.some((project) => !project.heading || !project.selected
    || project.pipelineRows < 10
    || project.holdRows !== 5
    || project.wiringPointCount < 1
    || project.physicalPointCount !== project.wiringPointCount
    || !project.normalizedWiring
    || project.wiringSurface.rows !== project.wiringPointCount
    || project.wiringSurface.unknownControllers !== 0
    || project.wiringSurface.missingCompiler !== 0
    || project.wiringSurface.inputSafeStateMismatch !== 0
    || project.executionVerdict !== 'proven'
    || project.sourceAuthoringVerdict !== 'not_proven'
    || project.unattendedVerdict !== 'not_proven'
    || !project.hilReviewStatus
    || !project.hilReviewReason
    || !project.hilStatusBound
    || !project.inspectorHilReasonVisible
    || project.inspectorBlockedPrerequisites < 1)
  || result.overview.pipelineRows < 10
  || result.overview.holdRows !== 5
  || result.overview.responsibilitySteps !== 4
  || !['agent-authoring', 'compiler-verification', 'physical-validation', 'release-authorization']
    .every((stage) => result.overview.responsibilityStages.includes(stage))
  || result.overview.agentAuthoringVerdict !== 'not_proven'
  || result.overview.humanOwnedStages !== 2
  || !['blocked', 'human_action_required', 'release_approved'].includes(result.overview.releaseStatus)
  || result.overview.viewportOverflowX
  || result.overview.viewportOverflowY
  || (result.topology.mode === 'missing' && (result.topology.blockerCode !== 'DELIVERY_GEOMETRY_ARTIFACT_MISSING' || !result.topology.blockerVisible))
  || (result.topology.mode === 'rendered' && (result.topology.nodeEvidenceRecords < 1 && result.topology.edgeEvidenceRecords < 1))
  || result.wiringRows !== 16
  || result.wiringDiagnostics !== 0
  || result.pointObservation.observationCountAfter !== result.pointObservation.observationCountBefore + 1
  || result.pointObservation.uploadCountAfter !== result.pointObservation.uploadCountBefore + 1
  || result.pointObservation.projectedStatus !== 'blocked'
  || result.pointObservation.latestStatus !== 'blocked'
  || !result.pointObservation.latestPhotoUploadId
  || result.signatureDialog.digestRows < 1
  || !result.signatureDialog.hasAttestation
  || result.signatureDialog.overflow
  || result.agentAudit.anomalyRows !== result.agentAudit.expectedAnomalies
  || result.agentAudit.correctionRows !== result.agentAudit.expectedCorrections
  || result.agentAudit.longSearchSignals < 1
  || result.paletteResultCount < 1
  || result.paletteFieldFilters.stageTokenResults.length < 1
  || result.paletteFieldFilters.diagnosticTokenResults.length < 1
  || result.paletteFieldFilters.evidenceResults.length < 1
  || result.paletteFieldFilters.projectResults.length !== 1
  || !result.paletteFieldFilters.paletteFocusRestored
  || result.evidenceFilterPersistence.stored !== 'blocked'
  || !result.evidenceFilterPersistence.restored
  || result.dimensionsAfter.explorer <= result.dimensionsBefore.explorer
  || result.dimensionsAfter.inspector <= result.dimensionsBefore.inspector
  || result.dimensionsAfter.bottom <= result.dimensionsBefore.bottom
  || result.dimensionsAfter.primary <= result.dimensionsBefore.primary
  || result.pointerResizeAfter < result.pointerResizeBefore + 12
  || !result.problemPanel.groupControl
  || !result.problemPanel.filterControl
  || result.problemPanel.rows < 1
  || result.problemCommitGroups.length < 1
  || !result.problemDeepLink.artifactPath?.endsWith('/anomalies.json')
  || result.problemDeepLink.artifactLine !== '1'
  || !result.testDeepLink.artifactPath?.endsWith('/compiler-stages.json')
  || result.testDeepLink.artifactLine !== '1'
  || result.groupedPanels.testRows < 1
  || !result.groupedPanels.testGroupControl
  || !result.groupedPanels.testFilterControl
  || result.splitSurface.separators < 4
  || result.splitSurface.groups !== 2
  || result.groupedPanels.separators < 3
  || result.groupedPanels.viewportOverflowX
  || result.groupedPanels.viewportOverflowY
));
console.log(JSON.stringify({ ok: !failed, results }, null, 2));
if (failed) process.exitCode = 1;
