ここでいう「W=8の有理数展開」を、(\rho_8) の厳密な分数

[
y=x^2
]

[
\rho_8=\frac{N_8}{D_8}
]

を得ることとして考えると、最大のボトルネックは行列生成でも最後のGMP処理でもなく、**必要桁数 (K) だけ逐次実行する p進リフト**です。

W=8のglide-even商では、

[
q=130040,\qquad n=q-1=130039
]

です。商行列は約67.64 GB、現在のexact-BLAS経路で元行列とLU因子を保持すると全rank合計約270.6 GBになります。64 rankなら行列部分は約4.23 GB/rankなので、メモリ自体は現実的です。

### 1. 必要p進桁数 (K)

現在の計算量は

[
O(n^3+Kn^2)
]

です。LU分解は一度だけですが、その後は各p進桁について、

1. LUを使った有限体連立方程式のsolve
2. 元行列との積による整数残差更新
3. 次の桁への完全除算

を逐次実行します。桁間に依存関係があるため、異なる桁を並列実行できません。

厳密な (K) は、まずこれを実行しないと分かりません。

```bash
./build/hpc-native/hexloop estimate-rho-direct \
  results/w8-glide-even.hlq \
  --prime 1048573 \
  --safety-digits 64 \
  -j 64 \
  -o results/w8-direct-bound.json
```

Cramer–Hadamard評価は、有理数再構成のために概ね係数上界の二乗以上の法を要求します。

かなり粗い構造上界として、各列ノルムを (2\cdot6^8) 以下と置くと、

[
\log_2 |\det A|
\lesssim 130039\log_2(2\cdot6^8)
\approx 2.82\times10^6\ \text{bits}
]

なので、対称有理数再構成には最大で約

[
5.64\times10^6\ \text{modulus bits}
]

すなわち (p=1048573) なら約28.2万桁が必要になる計算です。実際の列ノルムを使う estimator はこれより小さくなる可能性がありますが、**数万桁ではなく十数万～数十万桁になる可能性は十分あります**。

W=7でも、32768桁、法の大きさ655360 bitを使用し、定常ベクトルの最大座標は54924桁の十進整数でした。

### 2. 1桁ごとの分散solveとMPI同期

(n=130039) なので、

[
n^2 \approx 1.69\times10^{10}
]

です。1桁ごとに、この規模のLU solveと残差matvecが発生します。

block size 256の場合、前進・後退代入では概算

[
2\left\lceil \frac{130039}{256}\right\rceil=1016
]

回のblock broadcastが1桁ごとに必要です。元の実装資料でも、broadcast回数は (2\lceil n/b\rceil) とされています。

仮に28万桁なら、

[
1016\times 282000\approx2.86\times10^8
]

回のcollective処理になります。したがって、W=8では単純な演算性能よりも、

- MPI collectiveのレイテンシ
- rank間の負荷不均衡
- NUMA配置
- BLASスレッドとMPI rankの競合
- 1 RHSの三角solveがLevel-2 BLAS寄りになること

が実時間を決める可能性が高いです。

### 3. 最初の分散LU分解

一度だけとはいえ、dense LUの演算規模は概算

[
\frac23 n^3\approx1.47\times10^{15}
]

有限体更新です。

Schur complement部分はDGEMMに落とせますが、pivot探索、panel処理、modular reduction、MPI通信は通常の浮動小数点LUより重くなります。

さらに現時点の検証記録では、実MPIクラスタでの本番スケーリングはまだ確認されておらず、実施済みなのは主に1-rank互換環境と構文・小規模検証です。

実務上は以下の失敗もあります。

- 選んだ素数が行列式を割る
- block pivotingが適切なpivotを発見できない
- block sizeを大きくしすぎてpivot失敗
- exact-double条件を破る
- LU後のmod (p) residual verification失敗

したがって、28万桁を開始する前に、同じノード数・rank数で1-prime LUと8桁liftを完走させる必要があります。

### 4. 商行列の構築

glide-even商でも列数は130040で、各列について (6^8=1679616) 個の局所配置を合成するなら、処理する遷移数は概算

[
130040\times1679616
\approx2.18\times10^{11}
]

です。

現在のprobeでは約7.20 million transitions/sが観測されています。

この速度を単純に直列換算すると約8.4時間です。実際には並列化できますが、67.6 GBのdense出力への加算、キャッシュミス、checkpoint、MPI-IOが加わります。ただしこれは基本的に一回限りなので、長いp進liftよりは軽い可能性が高いです。

### 5. p進桁の保存

stream形式の保存量は

[
4nK\ \text{bytes}
]

です。

仮に (K=282000) なら、

[
4\times130039\times282000
\approx146.7\ \text{GB}
]

です。容量は問題になりにくいですが、

- checkpointごとの同期
- 並列ファイルシステムへの小さい書き込み
- resume時の全shard整合性確認
- rank障害による再計算

が問題になります。

現在のstreaming/checkpoint機構は小幅で検証されていますが、実際のmulti-rank W=8本番については未検証です。

### 6. 最終的な有理数再構成

(\rho) だけが必要なら、これは主要ボトルネックではありません。

`reconstruct-rho-direct` は全13万座標を個別に有理数再構成せず、

[
S=1+\sum_i x_i,\qquad
T=L_0+\sum_iL_ix_i
]

だけをp進的に集約して、

[
\rho_8=\frac{T}{8\cdot6^8S}
]

を一度だけ再構成します。全定常ベクトルのLCM、13万回の有理数再構成、巨大整数ベクトルの保持を回避しています。

ただし「(\rho_8) の分数」ではなく「定常ベクトル全体の有理数展開」が必要なら話は別です。W=7の全座標再構成だけでも約45172秒、約12.5時間かかっています。 W=8では座標数が約6.7倍になり、各座標の整数もさらに長くなるため、全ベクトル再構成は独立した重大ボトルネックになります。

### 結論

優先順位は次です。

1. `estimate-rho-direct` が返す必要桁数 (K)
2. (K) 回繰り返される分散LU solveと残差matvec
3. 1桁あたり約1016回のMPI同期
4. 最初の約(1.47\times10^{15})規模のLU分解
5. 67.6 GBの商行列構築
6. p進shard I/O
7. 最後のdirectな有理数再構成

実際に実行可能かを判断するために必要なのは、次の二つの数値です。

```text
K = estimate-rho-direct の minimum_digits
t_digit = 本番と同じ構成で8桁liftした際の秒/桁
```

おおよその総時間は、

[
T_{\mathrm{total}}
\approx T_{\mathrm{build}}
+T_{\mathrm{LU}}
+K,t_{\mathrm{digit}}
+T_{\mathrm{reconstruct}}
]

です。現状では、W=8の成否をほぼ決めるのは (K,t\_{\mathrm{digit}}) です。
