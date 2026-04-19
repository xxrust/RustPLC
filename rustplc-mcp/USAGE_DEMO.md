# RustPLC MCP Server 浣跨敤婕旂ず

鏈枃妗ｅ睍绀哄浣曞湪 Claude Code 涓娇鐢?RustPLC MCP 鏈嶅姟鍣ㄧ敓鎴愬拰楠岃瘉 PLC 绋嬪簭銆?
## 鍓嶇疆鍑嗗

1. **纭 MCP 鏈嶅姟鍣ㄥ凡閰嶇疆**
   - 椤圭洰鏍圭洰褰曞凡鏈?`.mcp.json` 閰嶇疆鏂囦欢
   - 閲嶅惎 Claude Code 鍚庝細鑷姩杩炴帴

2. **纭缂栬瘧鍣ㄥ凡鏋勫缓**
   ```bash
   cargo build --release
   ```

3. **杩愯娴嬭瘯纭涓€鍒囨甯?*
   ```bash
   cd rustplc-mcp
   python test_server.py
   ```

## 浣跨敤鍦烘櫙婕旂ず

### 鍦烘櫙 1锛氫粠闆剁敓鎴?PLC 绋嬪簭锛堝畬鏁存祦绋嬶級

**鐢ㄦ埛杈撳叆锛?*
```
甯垜鐢熸垚涓€涓弻缂搁『搴忓姩浣滅殑 PLC 绋嬪簭銆?
宸ヨ壓鎻忚堪锛?- 鏈変袱涓皵缂?A 鍜?B
- 鎸変笅鍚姩鎸夐挳鍚庯紝鍏堜几鍑?A 缂革紝绛夊緟鍒颁綅
- A 缂稿埌浣嶅悗锛屼几鍑?B 缂革紝绛夊緟鍒颁綅
- B 缂稿埌浣嶅悗锛屽厛缂╁洖 B 缂革紝绛夊緟鍒颁綅
- B 缂哥缉鍥炲悗锛岀缉鍥?A 缂革紝绛夊緟鍒颁綅
- 瀹屾垚鍚庡洖鍒扮瓑寰呭惎鍔ㄧ姸鎬?- 濡傛灉浠讳綍鍔ㄤ綔瓒呮椂锛岃繘鍏ユ晠闅滃鐞?```

**Claude Code 鎵ц娴佺▼锛?*

1. **璋冪敤 `get_rustplc_skill_guide` 宸ュ叿**
   - 鑾峰彇瀹屾暣鐨?SKILL.md 鍐呭
   - 浜嗚В鍥涢樁娈电敓鎴愭祦绋?
2. **闃舵闆讹細鐢熸垚 .system.md**
   ```markdown
   ## 椤圭洰韬唤
   - 椤圭洰鍚嶇О锛氬弻缂搁『搴忓姩浣滄紨绀虹郴缁?   - 鎵€灞炶涓氾細宸ヤ笟鑷姩鍖栨暀瀛?   - 閮ㄧ讲鍦烘墍锛氬疄楠屽鏁欏鍙?   - 鏈€缁堢敤鎴凤細鑷姩鍖栦笓涓氬鐢?   - 鐩戠瑕佹眰锛氭棤

   ## 绯荤粺浣垮懡
   婕旂ず姘旂几椤哄簭鎺у埗鐨勫熀鏈師鐞嗭紝鐢ㄤ簬鏁欏鍜屽疄楠屻€?   澶辨晥鍚庢灉锛氭紨绀哄け璐ワ紝鏃犱汉韬激瀹虫垨璐骇鎹熷け椋庨櫓銆?
   ## 瀹夊叏涓庡彲闈犳€у畾浣?   - 瀹夊叏绛夌骇锛氬父瑙勫伐涓氶槻鎶?   - 鏁呴殰鍚庢灉锛氭紨绀哄け璐?   - 瀹归敊绛栫暐锛氬崟涓€瓒呮椂淇濇姢

   ## 杩愯鐜
   - 浠嬭川锛氬帇缂╃┖姘?0.5MPa
   - 姘旀簮锛氬疄楠屽姘旀簮
   - 鐢垫簮锛?4V DC
   - 鎺у埗鍣細閫氱敤 PLC
   - 閫氫俊锛氭棤
   - 鐜鏉′欢锛氬鍐呭父娓?
   ## 鏍稿績宸ヨ壓鎰忓浘
   鎸夐挳鍚姩鍚庯紝A 缂镐几鍑?鈫?B 缂镐几鍑?鈫?B 缂哥缉鍥?鈫?A 缂哥缉鍥烇紝
   鍗曟寰幆锛屽畬鎴愬悗绛夊緟鍐嶆鍚姩銆?
   ## 鍏抽敭绾︽潫
   - A 缂稿拰 B 缂镐笉鑳藉悓鏃朵几鍑猴紙濡傛灉鏈夌墿鐞嗗共娑夛級
   - 姣忎釜鍔ㄤ綔蹇呴』鏈夎秴鏃朵繚鎶?   - 瓒呮椂鍚庤繘鍏ユ晠闅滃鐞嗭紝缂╁洖鎵€鏈夋皵缂?
   ## 璁捐鍋忓ソ
   - 鍛藉悕璇█锛氳嫳鏂?   - 浠ｇ爜椋庢牸锛氭竻鏅版槗璇?   - 鏃跺簭鍙傛暟锛氫娇鐢ㄩ粯璁ゅ€硷紙姘旂几 300ms锛岄榾 20ms锛?   - 鎵╁睍棰勬湡锛氭棤

   ## 瀵?AI 鐨勬寚寮?   杩欐槸鏁欏婕旂ず绯荤粺锛屽畨鍏ㄧ瓑绾т綆锛屼娇鐢ㄧ畝鍗曠殑瓒呮椂淇濇姢鍗冲彲銆?   涓嶉渶瑕佸啑浣欎紶鎰熷櫒鎴栧鏉傜殑瀹归敊閫昏緫銆?   ```

   **绛夊緟鐢ㄦ埛纭锛?* "杩欎釜绯荤粺鎻忚堪鍑嗙‘鍚楋紵"

3. **闃舵涓€锛氱悊瑙ｅ伐鑹?*
   ```
   鍔ㄤ綔搴忓垪锛?   1. [鍚姩鎸夐挳] 鎸変笅 鈫?瑙﹀彂寰幆
   2. [A 缂竇 浼稿嚭 鈫?绛夊緟 [A 缂镐几鍑轰紶鎰熷櫒]
   3. [B 缂竇 浼稿嚭 鈫?绛夊緟 [B 缂镐几鍑轰紶鎰熷櫒]
   4. [B 缂竇 缂╁洖 鈫?绛夊緟 [B 缂哥缉鍥炰紶鎰熷櫒]
   5. [A 缂竇 缂╁洖 鈫?绛夊緟 [A 缂哥缉鍥炰紶鎰熷櫒]
   6. 鍥炲埌绛夊緟鍚姩鐘舵€?
   瑙﹀彂鏂瑰紡锛氭寜閽惎鍔?   寰幆妯″紡锛氬崟娆″惊鐜紝瀹屾垚鍚庣瓑寰呭啀娆″惎鍔?   ```

   **纭闂锛?*
   - "鎴戠悊瑙ｇ殑鍔ㄤ綔椤哄簭瀵瑰悧锛?
   - "A 缂稿拰 B 缂稿彲浠ュ悓鏃跺姩浣滃悧锛?
   - "鏈夋病鏈夋垜閬楁紡鐨勫姩浣滄垨鐘舵€侊紵"

   **绛夊緟鐢ㄦ埛纭**

4. **闃舵浜岋細鎺ㄧ悊璁惧鎷撴墤**
   ```
   鎺ㄧ悊鍑虹殑璁惧鎷撴墤锛?
   PLC 鎺у埗鍣細
     - plc_main: plc { ports: [Y0, Y1, X0..X4] }

   鎵ц鏈烘瀯锛?     - A 缂?(cyl_A)    鈫?鐢电闃€ (valve_A)    鈫?plc_main.Y0
     - B 缂?(cyl_B)    鈫?鐢电闃€ (valve_B)    鈫?plc_main.Y1

   浼犳劅鍣細
     - A 缂镐几鍑?(sensor_A_ext)   鈫?plc_main.X0锛屾娴?cyl_A.extended
     - A 缂哥缉鍥?(sensor_A_ret)   鈫?plc_main.X1锛屾娴?cyl_A.retracted
     - B 缂镐几鍑?(sensor_B_ext)   鈫?plc_main.X2锛屾娴?cyl_B.extended
     - B 缂哥缉鍥?(sensor_B_ret)   鈫?plc_main.X3锛屾娴?cyl_B.retracted

   浜烘満浜や簰锛?     - 鍚姩鎸夐挳 (start_button)      鈫?plc_main.X4

   榛樿鏃跺簭鍙傛暟锛?     - 鐢电闃€鍝嶅簲: 20ms
     - 姘旂几琛岀▼: 300ms锛堜几鍑?缂╁洖锛?   ```

   **纭闂锛?*
   - "璁惧娓呭崟瀹屾暣鍚楋紵"
   - "鍛藉悕鍙互鍚楋紵"
   - "鏃跺簭鍙傛暟闇€瑕佽皟鏁村悧锛?

   **绛夊緟鐢ㄦ埛纭**

5. **闃舵涓夛細鎺ㄥ绾︽潫**
   ```
   鎺ㄥ鍑虹殑绾︽潫锛?
   瀹夊叏绾︽潫锛堢墿鐞嗗共娑夛級锛?     - 闇€瑕佺‘璁わ細A 缂稿拰 B 缂稿湪鐗╃悊涓婁細骞叉秹鍚楋紵
       濡傛灉浼氾細safety: cyl_A.extended conflicts_with cyl_B.extended

   鍥犳灉閾撅紙鑷姩鎺ㄦ柇锛夛細
     - Y0 -> valve_A -> cyl_A -> sensor_A_ext
     - Y1 -> valve_B -> cyl_B -> sensor_B_ext

   鏃跺簭绾︽潫锛?     - 鏁翠釜寰幆搴斿湪 3 绉掑唴瀹屾垚锛? 涓姩浣?脳 500ms 瓒呮椂锛?     - timing: task.cycle must_complete_within 3000ms
   ```

   **纭闂锛?*
   - "A 缂稿拰 B 缂稿湪鐗╃悊涓婁細骞叉秹鍚楋紵"
   - "杩樻湁鍏朵粬涓嶈兘鍚屾椂鍔ㄤ綔鐨勭粍鍚堝悧锛?
   - "鏁翠釜寰幆鏈夊懆鏈熸椂闂磋姹傚悧锛?

   **绛夊緟鐢ㄦ埛纭**

6. **闃舵鍥涳細鐢熸垚 DSL 骞堕獙璇?*

   鐢熸垚 `two_cylinder.plc` 鏂囦欢锛岀劧鍚庤皟鐢?`validate_plc` 宸ュ叿楠岃瘉銆?
   **濡傛灉楠岃瘉閫氳繃锛?*
   ```
   鉁?楠岃瘉閫氳繃

   楠岃瘉鎶ュ憡锛?   - Safety:    閫氳繃 鈥?conflicts_with 鍏ㄩ儴婊¤冻
   - Liveness:  閫氳繃 鈥?鏃犳閿侀闄?   - Timing:    閫氳繃 鈥?鍏抽敭璺緞 2640ms < 3000ms
   - Causality: 閫氳繃 鈥?鎵€鏈変俊鍙烽摼璺繛閫?   ```

   **濡傛灉楠岃瘉澶辫触锛?*
   - 闃呰閿欒淇℃伅
   - 淇闂
   - 閲嶆柊楠岃瘉
   - 鐩村埌鍏ㄩ儴閫氳繃

7. **鏈€缁堣緭鍑?*

   杩斿洖楠岃瘉閫氳繃鐨勫畬鏁?`.plc` 鏂囦欢鍜?`.system.md` 鏂囦欢銆?
---

### 鍦烘櫙 2锛氫娇鐢ㄦā鏉垮揩閫熺敓鎴?
**鐢ㄦ埛杈撳叆锛?*
```
/mcp__rustplc__two_cylinder_template button single
```

**Claude Code 鎵ц锛?*
- 璋冪敤 `two_cylinder_template` prompt
- 杩斿洖棰勫～鍏呯殑鍙岀几妯℃澘
- 璇㈤棶鏄惁闇€瑕佽皟鏁?- 璋冪敤 `validate_plc` 楠岃瘉

**杈撳嚭锛?*
瀹屾暣鐨?`.plc` 妯℃澘浠ｇ爜锛屽彲鐩存帴浣跨敤鎴栨牴鎹渶瑕佽皟鏁淬€?
---

### 鍦烘櫙 3锛氬弬鑰冪ず渚嬪涔?
**鐢ㄦ埛杈撳叆锛?*
```
鎴戞兂瀛︿範濡備綍浣跨敤 extern function 鍋氬鏉傝绠楋紝鏈夌ず渚嬪悧锛?```

**Claude Code 鎵ц锛?*
1. 璇诲彇 `@rustplc://examples/quadratic_fit.plc`
2. 璇诲彇 `@rustplc://docs/已实现/extern_function_mvp_spec.md`
3. 璇诲彇 `@rustplc://docs/已实现/extern_function_development_guide.md`
4. 缁撳悎涓夎€呯粰鍑鸿缁嗚瑙?
**杈撳嚭锛?*
- 瀹屾暣鐨勭ず渚嬩唬鐮?- 璇硶瑙勮寖璇存槑
- 寮€鍙戞寚鍗?- 浣跨敤寤鸿

---

### 鍦烘櫙 4锛氶獙璇佺幇鏈変唬鐮?
**鐢ㄦ埛杈撳叆锛?*
```
甯垜楠岃瘉杩欎釜 .plc 鏂囦欢锛?
[topology]
device plc_main: plc {
    purpose: "test",
    ports: [Y0:digital:producer, X0:digital:consumer]
}

[constraints]

[tasks]
task main:
    step wait:
        allow_indefinite_wait: true
```

**Claude Code 鎵ц锛?*
- 璋冪敤 `validate_plc` 宸ュ叿
- 浼犲叆浠ｇ爜鍐呭

**杈撳嚭锛?*
```
鉁?楠岃瘉閫氳繃

楠岃瘉鎶ュ憡锛?- Safety:    閫氳繃 鈥?鏃犲畨鍏ㄧ害鏉?- Liveness:  閫氳繃 鈥?allow_indefinite_wait 宸叉爣璁?- Timing:    閫氳繃 鈥?鏃犳椂搴忕害鏉?- Causality: 閫氳繃 鈥?鏃犲洜鏋滈摼
```

---

### 鍦烘櫙 5锛氱敓鎴?Extern Function 妯℃澘

**鐢ㄦ埛杈撳叆锛?*
```
/mcp__rustplc__extern_function_template quadratic_fit "x: float, y: float" "(float, float, float)" "math::fit" true 80
```

**Claude Code 鎵ц锛?*
- 璋冪敤 `extern_function_template` prompt
- 鐢熸垚瀹屾暣鐨勫０鏄庡拰璋冪敤绀轰緥

**杈撳嚭锛?*
```plc
[topology]

# 杈撳叆鍙橀噺
variable x: float = 0.0
variable y: float = 0.0

# 杈撳嚭鍙橀噺
variable out_0: float = 0.0
variable out_1: float = 0.0
variable out_2: float = 0.0

extern function quadratic_fit(x: float, y: float) -> (float, float, float) {
    rust_module: "math::fit"
    pure: true
    time_bound_us: 80
}

[tasks]

task main:
    step invoke:
        action: call quadratic_fit(x, y) -> (out_0, out_1, out_2)
        action: log "璋冪敤瀹屾垚"
    on_complete: goto done

task done:
    step hold:
        allow_indefinite_wait: true
```

---

## 鍙敤璧勬簮閫熸煡

### 璁块棶绀轰緥鏂囦欢
```
@rustplc://examples/two_cylinder.plc
@rustplc://examples/assembly_station.plc
@rustplc://examples/force_override_demo.plc
@rustplc://examples/nuclear_coolant_isolation.plc
@rustplc://examples/quadratic_fit.plc
```

### 璁块棶鎶€鏈枃妗?```
@rustplc://docs/已实现/extern_function_mvp_spec.md
@rustplc://docs/已实现/extern_function_development_guide.md
@rustplc://docs/dsl_verification_boundary.md
@rustplc://docs/device-library-design.md
@rustplc://docs/已实现/scenario_playbook.md
```

### 璁块棶鐢熸垚鎸囧紩
```
@rustplc://skill/rustplc              # 缁熶竴 skill 鎸囧紩
@rustplc://skill/rustplc/summary      # 绠€瑕佹憳瑕?```

## 璋冭瘯鎶€宸?
### 1. 鏌ョ湅 MCP 鏈嶅姟鍣ㄦ棩蹇?
濡傛灉閬囧埌闂锛屽彲浠ユ煡鐪?MCP 鏈嶅姟鍣ㄧ殑鏃ュ織杈撳嚭锛堝湪 Claude Code 鐨勭粓绔腑锛夈€?
### 2. 鎵嬪姩娴嬭瘯宸ュ叿

鍙互鍦?Python 涓墜鍔ㄦ祴璇曞伐鍏凤細

```python
cd rustplc-mcp
python

from tools.generate import *
from rust_bridge import validate_plc_content

# 娴嬭瘯楠岃瘉
plc_code = """
[topology]
device plc_main: plc {
    purpose: "test",
    ports: [X0:digital:consumer]
}

[constraints]

[tasks]
task main:
    step wait:
        allow_indefinite_wait: true
"""

result = validate_plc_content(plc_code)
print(result)
```

### 3. 妫€鏌ョ紪璇戝櫒璺緞

```python
from rust_bridge import RUSTPLC_BIN
print(f"RustPLC binary: {RUSTPLC_BIN}")
```

## 甯歌闂

### Q: MCP 鏈嶅姟鍣ㄦ棤娉曞惎鍔?
**A:** 妫€鏌ヤ互涓嬪嚑鐐癸細
1. 纭宸插畨瑁?`mcp` 鍖咃細`pip install mcp`
2. 纭 Python 鐗堟湰 >= 3.10
3. 妫€鏌?`.mcp.json` 涓殑璺緞鏄惁姝ｇ‘
4. 閲嶅惎 Claude Code

### Q: validate_plc 鎶ラ敊 "rustplc binary not found"

**A:** 妫€鏌ヤ互涓嬪嚑鐐癸細
1. 纭宸叉瀯寤虹紪璇戝櫒锛歚cargo build --release`
2. 妫€鏌?`.mcp.json` 涓殑 `RUSTPLC_PATH` 鏄惁姝ｇ‘
3. 鎵嬪姩璁剧疆鐜鍙橀噺锛歚export RUSTPLC_PATH=/path/to/rust_plc.exe`

### Q: 鏃犳硶璁块棶绀轰緥鏂囦欢

**A:** 妫€鏌ヤ互涓嬪嚑鐐癸細
1. 纭 `examples/` 鐩綍瀛樺湪浜庨」鐩牴鐩綍
2. 纭 `.mcp.json` 涓殑 `cwd` 璺緞姝ｇ‘
3. 杩愯 `python test_server.py` 妫€鏌ヨ祫婧愭槸鍚﹀彲璁块棶

### Q: 鐢熸垚鐨勪唬鐮侀獙璇佸け璐?
**A:** 杩欐槸姝ｅ父鐨勶紒MCP 鏈嶅姟鍣ㄤ細锛?1. 闃呰閿欒淇℃伅
2. 淇闂
3. 閲嶆柊楠岃瘉
4. 鐩村埌鍏ㄩ儴閫氳繃

濡傛灉澶氭澶辫触锛屽彲鑳芥槸锛?- 宸ヨ壓鎻忚堪涓嶆竻鏅帮紙闇€瑕佹洿澶氱‘璁わ級
- 绾︽潫鍐茬獊锛堥渶瑕佽皟鏁村畨鍏ㄧ害鏉燂級
- 鏃跺簭涓嶅悎鐞嗭紙闇€瑕佽皟鏁磋秴鏃跺€硷級

## 鎬荤粨

RustPLC MCP 鏈嶅姟鍣ㄦ彁渚涗簡涓€涓己澶ц€屾槗鐢ㄧ殑鎺ュ彛锛岃浣犲彲浠ワ細

1. 鉁?閫氳繃鑷劧璇█瀵硅瘽鐢熸垚 PLC 绋嬪簭
2. 鉁?鑷姩楠岃瘉鐢熸垚鐨勪唬鐮?3. 鉁?璁块棶鎵€鏈夌ず渚嬪拰鏂囨。
4. 鉁?浣跨敤棰勫畾涔夌殑妯℃澘蹇€熺敓鎴?5. 鉁?瀛︿範鍜屽弬鑰冩渶浣冲疄璺?
**寮€濮嬩娇鐢細** 鍦?Claude Code 涓洿鎺ュ璇濓紝璇村嚭浣犵殑闇€姹傦紝MCP 鏈嶅姟鍣ㄤ細寮曞浣犲畬鎴愭暣涓祦绋嬶紒

