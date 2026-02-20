export const en = {
  translation: {
    // Top Bar
    topBar: {
      save: 'Save',
      saving: 'Saving...',
      newTab: 'New tab',
      unsavedChanges: 'Unsaved changes',
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

    // Replay
    replay: {
      play: 'Play',
      pause: 'Pause',
      speed: 'Speed',
      tick: 'Tick',
      keypoints: 'Keypoints',
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
