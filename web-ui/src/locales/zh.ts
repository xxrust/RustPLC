export const zh = {
  translation: {
    // Top Bar
    topBar: {
      save: '保存',
      saving: '保存中...',
      newTab: '新建标签页',
      unsavedChanges: '未保存的更改',
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
    },

    // Component Library
    componentLibrary: {
      title: '组件库',
      cylinder: '气缸',
      sensor: '传感器',
      switch: '开关',
      stepper: '步进电机',
      generic: '通用',
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

    // Replay
    replay: {
      play: '播放',
      pause: '暂停',
      speed: '速度',
      tick: 'Tick',
      keypoints: '关键点',
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
