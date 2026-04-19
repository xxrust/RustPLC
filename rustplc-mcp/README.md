# RustPLC MCP Server

灏?RustPLC 缂栬瘧鍣ㄥ皝瑁呬负 MCP (Model Context Protocol) 鏈嶅姟鍣紝璁╀换浣?Claude Code 鐢ㄦ埛閫氳繃绠€鍗曢厤缃嵆鍙幏寰?PLC DSL 鐢熸垚鑳藉姏銆?
## 鍔熻兘鐗规€?
### Tools锛堝彲鎵ц宸ュ叿锛?
- **get_rustplc_skill_guide** - 鑾峰彇缁熶竴鐨?RustPLC skill 鎸囧紩锛圫KILL.md锛?- **validate_plc** - 楠岃瘉 .plc 鏂囦欢鏄惁閫氳繃鍥涘ぇ楠岃瘉寮曟搸锛圫afety/Liveness/Timing/Causality锛?- **compile_plc** - 缂栬瘧 .plc 鏂囦欢骞惰繑鍥?IR JSON 鍜岄獙璇佹姤鍛?
### Resources锛堟暟鎹簮锛?
- **rustplc://examples/\*** - 璁块棶鎵€鏈夌ず渚?.plc 鏂囦欢
- **rustplc://docs/\*** - 璁块棶鎶€鏈枃妗?- **rustplc://skill/rustplc** - 璁块棶缁熶竴鐨?RustPLC skill 鎸囧紩

### Prompts锛堝彲澶嶇敤妯℃澘锛?
- **generate_plc_from_requirements** - 浠庨渶姹傜敓鎴?.plc 绋嬪簭
- **two_cylinder_template** - 鍙屾皵缂搁『搴忓姩浣滄ā鏉?- **extern_function_template** - Extern 鍑芥暟澹版槑妯℃澘
- **pid_control_template** - PID 闂幆鎺у埗妯℃澘

## 蹇€熷紑濮?
### 鏂瑰紡涓€锛氭湰鍦板紑鍙戯紙鎺ㄨ崘锛?
1. **鏋勫缓 RustPLC 缂栬瘧鍣?*

```bash
cd /path/to/rust_plc
cargo build --release
```

2. **瀹夎 MCP 鏈嶅姟鍣ㄤ緷璧?*

```bash
cd rustplc-mcp
pip install mcp
```

3. **閰嶇疆 Claude Code**

鍦ㄩ」鐩牴鐩綍宸叉湁 `.mcp.json` 閰嶇疆鏂囦欢锛孋laude Code 浼氳嚜鍔ㄨ瘑鍒細

```json
{
  "mcpServers": {
    "rustplc": {
      "type": "stdio",
      "command": "python",
      "args": ["-m", "server"],
      "cwd": "${workspaceFolder}/rustplc-mcp",
      "env": {
        "RUSTPLC_PATH": "${workspaceFolder}/target/release/rust_plc"
      }
    }
  }
}
```

4. **閲嶅惎 Claude Code**

閲嶅惎鍚?MCP 鏈嶅姟鍣ㄤ細鑷姩杩炴帴銆?
### 鏂瑰紡浜岋細鍏ㄥ眬瀹夎锛堟湭鏉ユ敮鎸侊級

```bash
# 浠?PyPI 瀹夎锛堝緟鍙戝竷锛?pip install rustplc-mcp

# 娣诲姞鍒?Claude Code
claude mcp add --transport stdio rustplc -- python -m rustplc_mcp
```

## 浣跨敤绀轰緥

### 鍦烘櫙 1锛氫粠闆剁敓鎴?PLC 绋嬪簭

```
鐢ㄦ埛: "甯垜鐢熸垚涓€涓弻缂搁『搴忓姩浣滅殑 PLC 绋嬪簭"

Claude Code 浼氾細
1. 璋冪敤 get_rustplc_skill_guide 鑾峰彇鐢熸垚鎸囧紩
2. 鎵ц鍥涢樁娈垫祦绋嬶紙.system.md 鈫?鐞嗚В宸ヨ壓 鈫?鎺ㄧ悊鎷撴墤 鈫?鎺ㄥ绾︽潫 鈫?鐢熸垚 DSL锛?3. 姣忎釜闃舵閮戒細绛夊緟浣犵‘璁?4. 鏈€缁堢敓鎴?.plc 鏂囦欢骞惰嚜鍔ㄨ皟鐢?validate_plc 楠岃瘉
5. 杩斿洖楠岃瘉閫氳繃鐨勫畬鏁存枃浠?```

### 鍦烘櫙 2锛氫娇鐢ㄦā鏉垮揩閫熺敓鎴?
```
鐢ㄦ埛: "/mcp__rustplc__two_cylinder_template button single"

Claude Code 浼氾細
1. 杩斿洖棰勫～鍏呯殑鍙岀几妯℃澘
2. 璇㈤棶鏄惁闇€瑕佽皟鏁?3. 淇濆瓨鏂囦欢
```

### 鍦烘櫙 3锛氬弬鑰冪ず渚嬪涔?
```
鐢ㄦ埛: "鎴戞兂鐪嬬湅 PID 鎺у埗鎬庝箞鍐?

Claude Code 浼氾細
1. 璇诲彇 @rustplc://examples/force_override_demo.plc
2. 璇诲彇 @rustplc://docs/已实现/extern_function_mvp_spec.md
3. 缁撳悎涓よ€呯粰浣犺瑙?```

### 鍦烘櫙 4锛氶獙璇佺幇鏈変唬鐮?
```
鐢ㄦ埛: "甯垜楠岃瘉杩欎釜 .plc 鏂囦欢"
[绮樿创浠ｇ爜]

Claude Code 浼氾細
1. 璋冪敤 validate_plc 宸ュ叿
2. 杩斿洖璇︾粏鐨勯獙璇佹姤鍛?3. 濡傛灉澶辫触锛岀粰鍑轰慨澶嶅缓璁?```

## 鍙敤璧勬簮閫熸煡

### 绀轰緥鏂囦欢

```
@rustplc://examples/two_cylinder.plc              # 鍙屾皵缂搁『搴忓姩浣滐紙鍩虹锛?@rustplc://examples/assembly_station.plc          # 瑁呴厤绔欙紙澶氳澶囧崗鍚岋級
@rustplc://examples/force_override_demo.plc                  # PID 闂幆鎺у埗
@rustplc://examples/nuclear_coolant_isolation.plc # 鏍哥數绔欓殧绂婚榾锛圫IL3锛?@rustplc://examples/quadratic_fit.plc             # 浜屾鍑芥暟鎷熷悎锛堝鏉傝绠楋級
```

### 鎶€鏈枃妗?
```
@rustplc://docs/已实现/extern_function_mvp_spec.md           # Extern 鍑芥暟璇硶瑙勮寖
@rustplc://docs/已实现/extern_function_development_guide.md  # Extern 鍑芥暟寮€鍙戞寚鍗?@rustplc://docs/已实现/dsl_verification_boundary.md          # DSL 褰㈠紡鍖栭獙璇佽竟鐣?@rustplc://docs/已实现/device-library-design.md              # 璁惧搴撹璁?@rustplc://docs/已实现/scenario_playbook.md                  # 鍦烘櫙绯荤粺 playbook
```

### 鐢熸垚鎸囧紩

```
@rustplc://skill/rustplc         # 缁熶竴鐨?RustPLC skill 鎸囧紩
@rustplc://skill/rustplc/summary # 绠€瑕佹憳瑕?```

## 鍙敤 Prompts

### generate_plc_from_requirements

```
/mcp__rustplc__generate_plc_from_requirements "鎺ㄦ枡缂告妸宸ヤ欢鎺ㄥ埌浣嶏紝浼犳劅鍣ㄦ娴嬪埌鍚庡帇绱х几涓嬪帇"
```

### two_cylinder_template

```
/mcp__rustplc__two_cylinder_template button single
/mcp__rustplc__two_cylinder_template signal auto
```

鍙傛暟锛?- `start_mode`: "button"锛堟寜閽惎鍔級鎴?"signal"锛堜俊鍙峰惎鍔級
- `cycle_mode`: "single"锛堝崟娆″惊鐜級鎴?"auto"锛堣嚜鍔ㄥ惊鐜級

### extern_function_template

```
/mcp__rustplc__extern_function_template quadratic_fit "x: float, y: float" "(float, float, float)" "math::fit" true 80
```

鍙傛暟锛?- `func_name`: 鍑芥暟鍚?- `params`: 鍙傛暟鍒楄〃
- `return_type`: 杩斿洖绫诲瀷
- `rust_module`: Rust 妯″潡璺緞
- `pure`: 鏄惁绾嚱鏁帮紙true/false锛?- `time_bound_us`: 鏃堕棿涓婄晫锛堝井绉掞級

### pid_control_template

```
/mcp__rustplc__pid_control_template temperature 25.0 2.0 0.5 0.1
```

鍙傛暟锛?- `process_var`: 杩囩▼鍙橀噺鍚嶇О
- `setpoint`: 璁惧畾鍊?- `kp`: 姣斾緥绯绘暟
- `ki`: 绉垎绯绘暟
- `kd`: 寰垎绯绘暟

## 鏋舵瀯璇存槑

```
rustplc-mcp/
鈹溾攢鈹€ server.py              # MCP 鏈嶅姟鍣ㄥ叆鍙?鈹溾攢鈹€ rust_bridge.py         # Rust 缂栬瘧鍣ㄦˉ鎺ュ眰
鈹溾攢鈹€ tools/
鈹?  鈹溾攢鈹€ generate.py        # 鐢熸垚鍜岄獙璇佸伐鍏?鈹?  鈹斺攢鈹€ validate.py        # 楠岃瘉宸ュ叿锛堥鐣欙級
鈹溾攢鈹€ resources/
鈹?  鈹溾攢鈹€ examples.py        # 绀轰緥鏂囦欢璧勬簮
鈹?  鈹溾攢鈹€ docs.py            # 鏂囨。璧勬簮
鈹?  鈹斺攢鈹€ skill.py           # SKILL.md 璧勬簮
鈹溾攢鈹€ prompts/
鈹?  鈹斺攢鈹€ templates.py       # 鍦烘櫙妯℃澘
鈹溾攢鈹€ pyproject.toml         # Python 椤圭洰閰嶇疆
鈹斺攢鈹€ README.md              # 鏈枃浠?```

## 鐜鍙橀噺

- **RUSTPLC_PATH** - RustPLC 缂栬瘧鍣ㄤ簩杩涘埗璺緞锛堝彲閫夛紝榛樿鑷姩鏌ユ壘锛?
## 鏁呴殰鎺掓煡

### 闂锛歁CP 鏈嶅姟鍣ㄦ棤娉曞惎鍔?
**瑙ｅ喅鏂规锛?*
1. 纭宸插畨瑁?`mcp` 鍖咃細`pip install mcp`
2. 纭 Python 鐗堟湰 >= 3.10
3. 妫€鏌?`.mcp.json` 涓殑璺緞鏄惁姝ｇ‘

### 闂锛歷alidate_plc 鎶ラ敊 "rustplc binary not found"

**瑙ｅ喅鏂规锛?*
1. 纭宸叉瀯寤虹紪璇戝櫒锛歚cargo build --release`
2. 璁剧疆 `RUSTPLC_PATH` 鐜鍙橀噺鎸囧悜缂栬瘧鍣ㄤ簩杩涘埗
3. 鎴栧湪 `.mcp.json` 涓厤缃纭殑璺緞

### 闂锛氭棤娉曡闂ず渚嬫枃浠?
**瑙ｅ喅鏂规锛?*
1. 纭 `examples/` 鐩綍瀛樺湪浜庨」鐩牴鐩綍
2. 纭 `.mcp.json` 涓殑 `cwd` 璺緞姝ｇ‘

## 寮€鍙戞寚鍗?
### 娣诲姞鏂扮殑 Tool

鍦?`tools/` 鐩綍涓嬪垱寤烘柊鏂囦欢锛屽畾涔夊嚱鏁板苟鐢?`@mcp.tool()` 瑁呴グ锛?
```python
@mcp.tool()
def my_new_tool(param: str) -> str:
    """宸ュ叿鎻忚堪"""
    return f"Result: {param}"
```

### 娣诲姞鏂扮殑 Resource

鍦?`resources/` 鐩綍涓嬪垱寤烘柊鏂囦欢锛屽畾涔夊嚱鏁板苟鐢?`@mcp.resource()` 瑁呴グ锛?
```python
@mcp.resource("rustplc://my_resource/{id}")
def get_my_resource(id: str) -> str:
    """璧勬簮鎻忚堪"""
    return f"Resource content for {id}"
```

### 娣诲姞鏂扮殑 Prompt

鍦?`prompts/templates.py` 涓坊鍔犳柊鍑芥暟锛岀敤 `@mcp.prompt()` 瑁呴グ锛?
```python
@mcp.prompt()
def my_template(param: str) -> str:
    """妯℃澘鎻忚堪"""
    return f"Template with {param}"
```

## 璺嚎鍥?
### Phase 1: MVP锛堝凡瀹屾垚锛?- [x] Python FastMCP 鏈嶅姟鍣ㄦ鏋?- [x] Tool: get_rustplc_skill_guide, validate_plc, compile_plc
- [x] Resource: examples/\*, docs/\*, skill/rustplc
- [x] Prompt: 4 涓父瑙佸満鏅ā鏉?- [x] Stdio 浼犺緭鏀寔
- [x] 鍩虹鏂囨。

### Phase 2: 澧炲己锛堣鍒掍腑锛?- [ ] Tool: simulate_plc锛圫IL 浠跨湡锛?- [ ] Resource: device_library锛堣澶囧簱鏌ヨ锛?- [ ] HTTP 浼犺緭鏀寔
- [ ] Docker 闀滃儚
- [ ] PyPI 鍙戝竷

### Phase 3: 鐢熶骇鍖栵紙鏈潵锛?- [ ] 璁よ瘉鍜屾潈闄愭帶鍒?- [ ] 浣跨敤缁熻鍜岀洃鎺?- [ ] 閿欒澶勭悊鍜岄噸璇?- [ ] 缂撳瓨鍜屾€ц兘浼樺寲
- [ ] CI/CD 鑷姩鍙戝竷

## 璐＄尞鎸囧崡

娆㈣繋璐＄尞锛佽閬靛惊浠ヤ笅姝ラ锛?
1. Fork 鏈粨搴?2. 鍒涘缓鐗规€у垎鏀細`git checkout -b feature/my-feature`
3. 鎻愪氦鏇存敼锛歚git commit -am 'Add my feature'`
4. 鎺ㄩ€佸垎鏀細`git push origin feature/my-feature`
5. 鎻愪氦 Pull Request

## 璁稿彲璇?
涓?RustPLC 涓婚」鐩繚鎸佷竴鑷淬€?
## 鐩稿叧閾炬帴

- [RustPLC 涓讳粨搴揮(https://github.com/yourusername/rust_plc)
- [MCP 瀹樻柟鏂囨。](https://modelcontextprotocol.io/)
- [Claude Code 鏂囨。](https://code.claude.com/)

