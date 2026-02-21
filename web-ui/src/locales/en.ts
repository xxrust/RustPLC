export const en = {
  translation: {
    // Top Bar
    topBar: {
      save: 'Save',
      saving: 'Saving...',
      newTab: 'New tab',
      unsavedChanges: 'Unsaved changes',
      switchLanguage: '切换到中文',
      clickToSwitchProject: 'Click to switch project',
      noProject: '(no project)',
    },

    // Tabs
    tabs: {
      topology: 'Topology',
      replay: 'Tick Replay',
      scenario: 'Scenario',
      run: 'Run & Gate',
      diagnosis: 'Diagnosis',
      audit: 'Audit',
    },

    // Run Modes
    runMode: {
      no_board: 'No-Board',
      hil_board: 'HIL',
      runtime_live: 'Live',
    },

    // Status Bar
    statusBar: {
      connected: 'Connected',
      connectedWebSocket: 'Connected (WebSocket)',
      connectedPolling: 'Connected (Polling)',
      disconnected: 'Disconnected',
      noAlarms: 'No alarms',
      critical: 'critical',
      warning: 'warning',
      info: 'info',
      mode: 'Mode',
      version: 'RustPLC IDDE v1.0',
    },

    // Component Library
    componentLibrary: {
      title: 'Components',
      cylinder: 'Cylinder',
      sensor: 'Sensor',
      switch: 'Switch',
      stepper: 'Stepper Motor',
      generic: 'Generic',
      actuators: 'Actuators',
      sensors: 'Sensors',
      other: 'Other',
      searchPlaceholder: 'Search components...',
    },

    // Properties Panel
    properties: {
      title: 'Properties',
      canvasTitle: 'Topology Overview',
      cylinderTitle: 'Cylinder Properties',
      sensorTitle: 'Sensor Properties',
      switchTitle: 'Switch Properties',
      stepperTitle: 'Stepper Motor Properties',
      genericTitle: 'Generic Node Properties',

      // Common fields
      label: 'Label',
      status: 'Status',
      value: 'Value',
      save: 'Save',
      revert: 'Revert',

      // Cylinder
      responseTime: 'Response Time (ms)',
      statusRetracted: 'Retracted',
      statusExtended: 'Extended',
      statusMoving: 'Moving',
      statusFault: 'Fault',

      // Sensor
      statusOn: 'On',
      statusOff: 'Off',
      detects: 'Detects (Target Node ID)',
      detectsPlaceholder: 'e.g., cylinder_1',

      // Switch
      statusOpen: 'Open',
      statusClosed: 'Closed',

      // Stepper
      direction: 'Direction',
      directionForward: 'Forward',
      directionReverse: 'Reverse',
      directionStopped: 'Stopped',
      enable: 'Enable',
      position: 'Position (steps)',
      stepsPerRev: 'Steps per Revolution',

      // Generic
      keyValueEditor: 'Key-Value Editor',
      addField: '+ Add Field',

      // Canvas
      statistics: 'Statistics',
      totalNodes: 'Total Nodes',
      totalEdges: 'Total Edges',
      nodeTypes: 'Node Types',
      noNodes: 'No nodes in topology',
      instructions: 'Instructions',
      instructionDrag: '• Drag components from the library to add nodes',
      instructionConnect: '• Connect nodes by dragging from handles',
      instructionSelect: '• Select a node to edit its properties',
      instructionDelete: '• Press Delete to remove selected nodes/edges',
      instructionRightClick: '• Right-click nodes for fault injection',

      // Tag batch editor
      batchTitle: 'Tag Batch Refactor',
      batchDimension: 'Tag Dimension',
      batchFilter: 'Tag Filter',
      batchFilterPlaceholder: 'e.g. conveyor, high, line_a/cell_2 or *',
      batchFilterHintEmpty: 'Enter a tag filter to preview impacted nodes.',
      batchFilterHint: '{{count}} node(s) currently match this filter.',
      batchNodePatch: 'Node Patch (JSON object)',
      batchRename: 'Naming Rule',
      batchRenamePrefix: 'Prefix',
      batchRenameSuffix: 'Suffix',
      batchRenameSearch: 'Replace from',
      batchRenameReplace: 'Replace to',
      batchEdgeUpdate: 'Edge Update',
      batchEdgeScopeTouched: 'Touched edges',
      batchEdgeScopeInternal: 'Internal edges',
      batchEdgeSignalKeep: 'Keep signal labels',
      batchEdgeSignalSet: 'Set signal label',
      batchEdgeSignalClear: 'Clear signal label',
      batchEdgeSignalPlaceholder: 'Signal label value',
      batchPreview: 'Preview Diff',
      batchApply: 'Apply Batch',
      batchRollback: 'Rollback Last Batch',
      batchExport: 'Export Topology JSON',
      batchWriteBack: 'Write Back',
      batchWriteBackSaving: 'Writing...',
      batchPreviewError: 'Unable to build batch preview.',
      batchApplySuccess: 'Batch changes applied. Review and save when ready.',
      batchRollbackSuccess: 'Last batch change has been rolled back.',
      batchExportSuccess: 'Topology JSON exported.',
      batchWriteBackNeedProject: 'Select a project before writing back.',
      batchWriteBackSuccess: 'Topology written back to project.',
      batchWriteBackFailed: 'Write-back failed. Check server/API status.',
      batchPreviewSummary:
        'Preview: matched {{matched}} node(s), node changes {{nodeChanges}}, edge changes {{edgeChanges}}.',
      batchPreviewNodeChanges: 'Node updates',
      batchPreviewEdgeChanges: 'Edge updates',

      // Tag view controls
      tagViewTitle: 'Tag View Controls',
      tagViewFilterDimension: 'Filter by tag dimension',
      tagViewFilterPlaceholder: 'Filter by exact tag or use *',
      tagViewFilterSummary: 'Visible after filter: {{nodes}} node(s), {{edges}} edge(s).',
      tagViewClearFilter: 'Clear Filter',
      tagViewGroupingDimension: 'Highlight groups by',
      tagViewGroupingToggle: 'Enable grouped highlighting',
      tagViewGroupingMore: '+{{count}} more groups',
      tagViewClearGrouping: 'Clear Highlight',
      tagViewLocationLocate: 'Locate fault region (location_group)',
      tagViewLocationPlaceholder: 'e.g. line_a/cell_2/station_7',
      tagViewLocationPreview:
        'Region matches {{region}} node(s), with neighbors {{neighbors}} node(s).',
      tagViewLocateButton: 'Locate Region + Neighbors',
      tagViewClearLocate: 'Clear Locate',
      tagViewLocationActive:
        'Focused region: {{region}} node(s), with neighbors {{neighbors}} node(s).',
    },

    // Context Menu
    contextMenu: {
      injectJammed: 'Inject: Jammed',
      injectMotionTimeout: 'Inject: Motion Timeout',
      injectStuckOn: 'Inject: Stuck On',
      injectStuckOff: 'Inject: Stuck Off',
      injectChatter: 'Inject: Chatter',
      injectLostStep: 'Inject: Lost Step',
      injectStall: 'Inject: Stall',
      injectDirectionReversed: 'Inject: Direction Reversed',
      clearFaults: 'Clear Faults',
      deleteNode: 'Delete Node',
      deleteConfirm: 'Delete safety-critical node? This action cannot be undone.',
    },

    // Login Page
    login: {
      title: 'RustPLC Web UI',
      subtitle: 'Industrial Control System IDDE',
      username: 'Username',
      usernamePlaceholder: 'Enter username',
      password: 'Password',
      passwordPlaceholder: 'Enter password',
      loginButton: 'Login',
      loggingIn: 'Logging in...',
      demoCredentials: 'Demo Credentials:',
      errorRequired: 'Username and password are required',
      errorFailed: 'Login failed: Invalid credentials',
    },

    // Protected Route
    protectedRoute: {
      accessDenied: 'Access Denied',
      noPermission: 'You do not have permission to access this page.',
      requiredRole: 'Required role',
      yourRole: 'Your role',
      goBack: 'Go Back',
    },

    // Validation Errors
    validation: {
      title: 'Validation Failed',
      errorsFound: 'error',
      errorsFoundPlural: 'errors found',
      close: 'Close',
    },

    // Placeholder Views
    placeholders: {
      scenario: 'Scenario / Recipe',
      scenarioDesc: 'Scenario YAML editor and visual timeline — Phase 2',
      run: 'Run & Gate',
      runDesc: 'Trigger no-board-gate, commissioning-run, trace-doctor — Phase 2',
      diagnosis: 'Alarm & Diagnosis',
      diagnosisDesc: 'Real-time alarms and diagnosis report — Phase 2',
      audit: 'Audit & Reports',
      auditDesc: 'Audit log and report export — Phase 3',
    },

    // IDDE Layout
    idde: {
      showSidebar: 'Show sidebar',
      showProperties: 'Show properties',
      noProjectSelected: 'No project selected',
    },

    // Replay
    replay: {
      play: 'Play',
      pause: 'Pause',
      speed: 'Speed',
      tick: 'Tick',
      keypoints: 'Keypoints',
      prevKeypoint: 'Prev keypoint',
      nextKeypoint: 'Next keypoint',
      stepBack: 'Step back',
      stepForward: 'Step forward',
      errorAtTick: 'Error at tick',
      eventAtTick: 'Event at tick',
    },

    // Common
    common: {
      loading: 'Loading...',
      error: 'Error',
      success: 'Success',
      cancel: 'Cancel',
      confirm: 'Confirm',
      delete: 'Delete',
      edit: 'Edit',
      add: 'Add',
      remove: 'Remove',
    },

    // Diagnosis
    diagnosis: {
      title: 'Diagnosis Center',
      severity: 'Severity',
      alarmId: 'Alarm ID',
      firstSeen: 'First Seen',
      scenario: 'Scenario / Recipe',
      evidenceSource: 'Evidence Source',
      actions: 'Actions',
      viewDetails: 'View Details',
      acknowledge: 'Acknowledge',
      acknowledgeAlarm: 'Acknowledge Alarm',
      alarmList: 'Alarm List',
      alarmDetails: 'Alarm Details',
      acknowledged: 'acknowledged',
      ackSuccess: 'Alarm acknowledged',
      ackFailed: 'Failed to acknowledge',
      candidates: 'Diagnosis Candidates',
      issueCode: 'Issue Code',
      rank: 'Rank',
      confidence: 'Confidence',
      category: 'Category',
      evidence: 'Evidence',
      suggestedFix: 'Suggested Fix',
      evidenceRef: 'Evidence Ref',
    },

    // Main Layout
    mainLayout: {
      profile: 'Profile',
      settings: 'Settings',
      logout: 'Logout',
    },

    // Topology Page
    topologyPage: {
      title: 'Topology Editor',
      validate: 'Validate',
      validateSuccess: 'Topology validation passed',
      validateFailed: 'Validation failed',
      saveSuccess: 'Topology saved',
      jsonError: 'Invalid JSON format',
      plcCode: 'PLC Code',
      plcFile: 'PLC File',
      placeholder: 'PLC code...',
      visualEditor: 'Visual Editor (WIP)',
      visualEditorWip: 'Visual topology editor coming soon...',
      visualEditorPlan: 'Planned: drag-and-drop components, wiring, property config, live validation',
    },

    // Scenario Page
    scenarioPage: {
      title: 'Scenario Manager',
      validate: 'Validate',
      validateSuccess: 'Scenario validation passed',
      validateFailed: 'Validation failed',
      saveSuccess: 'Scenario saved',
      jsonError: 'Invalid JSON format',
      scenarioFile: 'Scenario File',
      placeholder: 'Scenario YAML or JSON...',
      visualEditor: 'Visual Editor (WIP)',
      visualEditorWip: 'Visual scenario editor coming soon...',
      visualEditorPlan: 'Planned: timeline editing, event drag-and-drop, fault injection config',
    },

    // Replay Page (full page, not timeline)
    replayPage: {
      selectRun: 'Select Run',
      selectRunPlaceholder: 'Select a run record',
      playbackControl: 'Playback Control',
      prevFrame: 'Prev Frame',
      nextFrame: 'Next Frame',
      playSpeed: 'Play Speed',
      currentTick: 'Current Tick',
      timeMs: 'Time (ms)',
      digitalInputs: 'Digital Inputs',
      digitalOutputs: 'Digital Outputs',
      digitalSignals: 'Digital Signals',
      analogSignals: 'Analog Signals',
      inputs: 'Inputs',
      outputs: 'Outputs',
      signal: 'Signal',
      state: 'State',
    },

    // Dashboard
    dashboard: {
      title: 'Overview',
      runMode: 'Run Mode',
      currentProject: 'Current Project',
      latestRunStatus: 'Latest Run Status',
      alarmCount: 'Alarm Count',
      quickAccess: 'Quick Access',
      runGate: 'Run Gate',
      auditReport: 'Audit Report',
      recentRuns: 'Recent Runs',
      recentAlarms: 'Recent Alarms',
      viewAll: 'View All',
    },

    // Run Page
    run: {
      title: 'Run Monitor',
      triggerGate: 'Trigger No-Board Gate',
      plcFile: 'PLC File',
      plcFileRequired: 'Please enter PLC file path',
      scenarioFile: 'Scenario File',
      scenarioFileRequired: 'Please enter scenario file path',
      run: 'Run',
      triggered: 'Run triggered',
      triggerFailed: 'Run failed',
      runHistory: 'Run History',
      refresh: 'Refresh',
      runDetails: 'Run Details',
      runId: 'Run ID',
      status: 'Status',
      triggeredBy: 'Triggered By',
      triggeredAt: 'Triggered At',
      failureSummary: 'Failure Summary',
      actions: 'Actions',
      viewDetails: 'View Details',
      diagnosis: 'Diagnosis',
      artifacts: 'Artifacts',
      traceData: 'Trace Data',
      diffReport: 'Diff Report',
      timingReport: 'Timing Report',
      diagnosisReport: 'Diagnosis Report',
    },

    // Project Selector
    projectSelector: {
      title: 'Select Project',
      select: 'Select Project',
      current: 'Current Project',
      selectNew: 'Select New Project',
      placeholder: 'Select a PLC project',
      path: 'Path',
      switched: 'Switched to project',
      openLocal: 'Open local .plc file',
      browseFile: 'Browse file...',
      orFromServer: 'or from server',
    },

    // Canvas Controls
    canvas: {
      zoomIn: 'Zoom In',
      zoomOut: 'Zoom Out',
      fitView: 'Fit View',
      lockView: 'Lock View',
      unlockView: 'Unlock View',
      portFallbackNodesNotice:
        '{{count}} node(s) are using fallback port contracts; edge binding is degraded.',
      portFallbackEdgeWarning:
        'Missing explicit port metadata; edge added in degraded mode.',
      portTypeMismatch: 'Port type mismatch: {{source}} -> {{target}}.',
      portRoleMismatch: 'Connection direction does not match port role.',
      portBindingUnavailable:
        'Cannot bind edge to unique source/target handles for this node pair.',
      dismissWarning: 'Dismiss warning',
    },

    // Notifications
    notifications: {
      saveSuccess: 'Topology saved successfully',
      saveFailed: 'Failed to save topology',
      injectSuccess: 'Fault injected successfully',
      injectFailed: 'Failed to inject fault',
      clearSuccess: 'Faults cleared successfully',
      clearFailed: 'Failed to clear faults',
      toggleSuccess: 'State toggled successfully',
      toggleFailed: 'Failed to toggle state',
    },
  },
};
