export const zh = {
  translation: {
    // Top Bar
    topBar: {
      save: '保存',
      saving: '保存中...',
      newTab: '新建标签页',
      unsavedChanges: '未保存的更改',
      switchLanguage: 'Switch to English',
      clickToSwitchProject: '点击切换项目',
      noProject: '(未选择项目)',
    },

    // Tabs
    tabs: {
      topology: '拓扑图',
      replay: 'Tick 回放',
      scenario: '场景',
      run: '运行与门禁',
      diagnosis: '诊断',
      audit: '审计',
    },

    // Run Modes
    runMode: {
      no_board: '无板',
      hil_board: 'HIL',
      runtime_live: '实时',
    },

    // Status Bar
    statusBar: {
      connected: '已连接',
      connectedWebSocket: '已连接 (WebSocket)',
      connectedPolling: '已连接 (轮询)',
      disconnected: '已断开',
      noAlarms: '无告警',
      critical: '严重',
      warning: '警告',
      info: '信息',
      mode: '模式',
      version: 'RustPLC IDDE v1.0',
    },

    // Component Library
    componentLibrary: {
      title: '组件库',
      cylinder: '气缸',
      sensor: '传感器',
      switch: '开关',
      stepper: '步进电机',
      generic: '通用',
      actuators: '执行器',
      sensors: '传感器',
      other: '其他',
      searchPlaceholder: '搜索组件...',
    },

    // Properties Panel
    properties: {
      title: '属性',
      canvasTitle: '拓扑概览',
      cylinderTitle: '气缸属性',
      sensorTitle: '传感器属性',
      switchTitle: '开关属性',
      stepperTitle: '步进电机属性',
      genericTitle: '通用节点属性',

      // Common fields
      label: '标签',
      status: '状态',
      value: '值',
      save: '保存',
      revert: '还原',

      // Cylinder
      responseTime: '响应时间 (ms)',
      statusRetracted: '收回',
      statusExtended: '伸出',
      statusMoving: '运动中',
      statusFault: '故障',

      // Sensor
      statusOn: '开',
      statusOff: '关',
      detects: '检测目标 (节点 ID)',
      detectsPlaceholder: '例如：cylinder_1',

      // Switch
      statusOpen: '断开',
      statusClosed: '闭合',

      // Stepper
      direction: '方向',
      directionForward: '正向',
      directionReverse: '反向',
      directionStopped: '停止',
      enable: '使能',
      position: '位置 (步数)',
      stepsPerRev: '每转步数',

      // Generic
      keyValueEditor: '键值编辑器',
      addField: '+ 添加字段',

      // Canvas
      statistics: '统计信息',
      totalNodes: '节点总数',
      totalEdges: '连接总数',
      nodeTypes: '节点类型',
      noNodes: '拓扑中无节点',
      instructions: '操作说明',
      instructionDrag: '• 从组件库拖拽组件以添加节点',
      instructionConnect: '• 从连接点拖拽以连接节点',
      instructionSelect: '• 选择节点以编辑其属性',
      instructionDelete: '• 按 Delete 键删除选中的节点/连接',
      instructionRightClick: '• 右键点击节点进行故障注入',

      // Tag batch editor
      batchTitle: '标签批量改造',
      batchDimension: '标签维度',
      batchFilter: '标签筛选',
      batchFilterPlaceholder: '例如 conveyor、high、line_a/cell_2 或 *',
      batchFilterHintEmpty: '输入标签筛选后可预览受影响节点。',
      batchFilterHint: '当前有 {{count}} 个节点命中该筛选。',
      batchNodePatch: '节点批量补丁（JSON 对象）',
      batchRename: '命名规则',
      batchRenamePrefix: '前缀',
      batchRenameSuffix: '后缀',
      batchRenameSearch: '替换来源',
      batchRenameReplace: '替换目标',
      batchEdgeUpdate: '连线批量更新',
      batchEdgeScopeTouched: '所有关联连线',
      batchEdgeScopeInternal: '仅筛选集内部连线',
      batchEdgeSignalKeep: '保持 signal 标签',
      batchEdgeSignalSet: '设置 signal 标签',
      batchEdgeSignalClear: '清空 signal 标签',
      batchEdgeSignalPlaceholder: 'signal 标签值',
      batchPreview: '预览 Diff',
      batchApply: '应用批量修改',
      batchRollback: '回滚上一次批量修改',
      batchExport: '导出拓扑 JSON',
      batchWriteBack: '写回项目',
      batchWriteBackSaving: '写回中...',
      batchPreviewError: '生成批量预览失败。',
      batchApplySuccess: '批量修改已应用，请确认后保存。',
      batchRollbackSuccess: '已回滚上一次批量修改。',
      batchExportSuccess: '拓扑 JSON 已导出。',
      batchWriteBackNeedProject: '请先选择项目再写回。',
      batchWriteBackSuccess: '拓扑已写回项目。',
      batchWriteBackFailed: '写回失败，请检查服务端/API 状态。',
      batchPreviewSummary:
        '预览：命中节点 {{matched}} 个，节点变更 {{nodeChanges}} 处，连线变更 {{edgeChanges}} 处。',
      batchPreviewNodeChanges: '节点变更',
      batchPreviewEdgeChanges: '连线变更',
    },

    // Context Menu
    contextMenu: {
      injectJammed: '注入：卡死',
      injectMotionTimeout: '注入：运动超时',
      injectStuckOn: '注入：卡在开启',
      injectStuckOff: '注入：卡在关闭',
      injectChatter: '注入：抖动',
      injectLostStep: '注入：丢步',
      injectStall: '注入：堵转',
      injectDirectionReversed: '注入：方向反转',
      clearFaults: '清除故障',
      deleteNode: '删除节点',
      deleteConfirm: '删除安全关键节点？此操作无法撤销。',
    },

    // Login Page
    login: {
      title: 'RustPLC Web UI',
      subtitle: '工业控制系统 IDDE',
      username: '用户名',
      usernamePlaceholder: '请输入用户名',
      password: '密码',
      passwordPlaceholder: '请输入密码',
      loginButton: '登录',
      loggingIn: '登录中...',
      demoCredentials: '演示账号：',
      errorRequired: '用户名和密码为必填项',
      errorFailed: '登录失败：用户名或密码错误',
    },

    // Protected Route
    protectedRoute: {
      accessDenied: '访问被拒绝',
      noPermission: '您没有权限访问此页面。',
      requiredRole: '所需角色',
      yourRole: '您的角色',
      goBack: '返回',
    },

    // Validation Errors
    validation: {
      title: '验证失败',
      errorsFound: '个错误',
      errorsFoundPlural: '个错误',
      close: '关闭',
    },

    // Placeholder Views
    placeholders: {
      scenario: '场景 / 配方',
      scenarioDesc: '场景 YAML 编辑器和可视化时间线 — 第二阶段',
      run: '运行与门禁',
      runDesc: '触发无板门禁、调试运行、trace-doctor — 第二阶段',
      diagnosis: '告警与诊断',
      diagnosisDesc: '实时告警和诊断报告 — 第二阶段',
      audit: '审计与报告',
      auditDesc: '审计日志和报告导出 — 第三阶段',
    },

    // IDDE Layout
    idde: {
      showSidebar: '显示侧边栏',
      showProperties: '显示属性面板',
      noProjectSelected: '未选择项目',
    },

    // Replay
    replay: {
      play: '播放',
      pause: '暂停',
      speed: '速度',
      tick: 'Tick',
      keypoints: '关键点',
      prevKeypoint: '上一关键点',
      nextKeypoint: '下一关键点',
      stepBack: '后退一帧',
      stepForward: '前进一帧',
      errorAtTick: '错误于 Tick',
      eventAtTick: '事件于 Tick',
    },

    // Common
    common: {
      loading: '加载中...',
      error: '错误',
      success: '成功',
      cancel: '取消',
      confirm: '确认',
      delete: '删除',
      edit: '编辑',
      add: '添加',
      remove: '移除',
    },

    // Diagnosis
    diagnosis: {
      title: '诊断中心',
      severity: '严重程度',
      alarmId: '告警 ID',
      firstSeen: '首次发现',
      scenario: '场景/配方',
      evidenceSource: '证据来源',
      actions: '操作',
      viewDetails: '查看详情',
      acknowledge: '确认',
      acknowledgeAlarm: '确认告警',
      alarmList: '告警列表',
      alarmDetails: '告警详情',
      acknowledged: '已确认',
      ackSuccess: '告警已确认',
      ackFailed: '确认失败',
      candidates: '诊断候选项',
      issueCode: '问题代码',
      rank: '排名',
      confidence: '置信度',
      category: '类别',
      evidence: '证据',
      suggestedFix: '建议修复',
      evidenceRef: '证据引用',
    },

    // Main Layout
    mainLayout: {
      profile: '个人信息',
      settings: '设置',
      logout: '退出登录',
    },

    // Topology Page
    topologyPage: {
      title: '拓扑编辑器',
      validate: '验证',
      validateSuccess: '拓扑验证通过',
      validateFailed: '验证失败',
      saveSuccess: '拓扑已保存',
      jsonError: 'JSON 格式错误',
      plcCode: 'PLC 代码',
      plcFile: 'PLC 文件',
      placeholder: 'PLC 代码...',
      visualEditor: '可视化编辑（开发中）',
      visualEditorWip: '可视化拓扑编辑器开发中...',
      visualEditorPlan: '功能规划：拖拽组件、连线编辑、属性配置、实时验证',
    },

    // Scenario Page
    scenarioPage: {
      title: '场景管理器',
      validate: '验证',
      validateSuccess: '场景验证通过',
      validateFailed: '验证失败',
      saveSuccess: '场景已保存',
      jsonError: 'JSON 格式错误',
      scenarioFile: '场景文件',
      placeholder: '场景 YAML 或 JSON...',
      visualEditor: '可视化编辑（开发中）',
      visualEditorWip: '可视化场景编辑器开发中...',
      visualEditorPlan: '功能规划：时间线编辑、事件拖拽、故障注入配置',
    },

    // Replay Page (full page, not timeline)
    replayPage: {
      selectRun: '选择运行记录',
      selectRunPlaceholder: '选择运行记录',
      playbackControl: '播放控制',
      prevFrame: '上一帧',
      nextFrame: '下一帧',
      playSpeed: '播放速度',
      currentTick: '当前 Tick',
      timeMs: '时间 (ms)',
      digitalInputs: '数字输入',
      digitalOutputs: '数字输出',
      digitalSignals: '数字信号',
      analogSignals: '模拟信号',
      inputs: '输入',
      outputs: '输出',
      signal: '信号',
      state: '状态',
    },

    // Dashboard
    dashboard: {
      title: '总览看板',
      runMode: '运行模式',
      currentProject: '当前项目',
      latestRunStatus: '最新运行状态',
      alarmCount: '告警数量',
      quickAccess: '快速入口',
      runGate: '运行门禁',
      auditReport: '审计报告',
      recentRuns: '最近运行记录',
      recentAlarms: '最新告警',
      viewAll: '查看全部',
    },

    // Run Page
    run: {
      title: '运行监控',
      triggerGate: '触发 No-Board Gate',
      plcFile: 'PLC 文件',
      plcFileRequired: '请输入 PLC 文件路径',
      scenarioFile: '场景文件',
      scenarioFileRequired: '请输入场景文件路径',
      run: '运行',
      triggered: '运行已触发',
      triggerFailed: '运行失败',
      runHistory: '运行记录',
      refresh: '刷新',
      runDetails: '运行详情',
      runId: '运行 ID',
      status: '状态',
      triggeredBy: '触发人',
      triggeredAt: '触发时间',
      failureSummary: '失败原因',
      actions: '操作',
      viewDetails: '查看详情',
      diagnosis: '诊断',
      artifacts: '工件',
      traceData: 'Trace 数据',
      diffReport: 'Diff 报告',
      timingReport: '时序报告',
      diagnosisReport: '诊断报告',
    },

    // Project Selector
    projectSelector: {
      title: '选择项目',
      select: '选择项目',
      current: '当前项目',
      selectNew: '选择新项目',
      placeholder: '选择一个 PLC 项目',
      path: '路径',
      switched: '已切换到项目',
      openLocal: '打开本地 .plc 文件',
      browseFile: '浏览文件...',
      orFromServer: '或从服务器选择',
    },

    // Canvas Controls
    canvas: {
      zoomIn: '放大',
      zoomOut: '缩小',
      fitView: '适应视图',
      lockView: '锁定视图',
      unlockView: '解锁视图',
    },

    // Notifications
    notifications: {
      saveSuccess: '拓扑保存成功',
      saveFailed: '拓扑保存失败',
      injectSuccess: '故障注入成功',
      injectFailed: '故障注入失败',
      clearSuccess: '故障清除成功',
      clearFailed: '故障清除失败',
      toggleSuccess: '状态切换成功',
      toggleFailed: '状态切换失败',
    },
  },
};
