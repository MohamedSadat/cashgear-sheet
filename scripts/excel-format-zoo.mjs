// Writes workbooks holding every built-in number format, every harvested code and a hand-written
// list of hard ones to tests/workbooks/generated/format-zoo, which git ignores. Excel opens them
// read-only over COM, and what it displays goes to crates/cgs-numfmt/tests/data/excel-text.jsonl,
// the reference for the crate's differential test. The public workbooks are never opened: only
// their format code strings are used, through scripts/harvest-format-codes.mjs.
//
// Which codes Excel refuses is found by opening files, not through Range.NumberFormat: that takes
// colour names in Office's own language, and this PC's Office UI is Arabic on en-US Windows, so it
// refuses [Red]. Windows' language decides the rest of what is measured here, so run this on en-US.
//
// The output's first line holds the value sets. Every other line is one code applied to one set:
//   {"group":"hand","code":"0.00","set":"core","text":[...],"fn":[...]}
// `text` is the cell's .Text for each value in the set, and `fn` is =TEXT(value,code) for each of
// the `fn` values. `invalid` marks a code Excel refuses, and `builtin` a built-in id. A run of 50
// or more equal characters that aren't digits, which only a fill or #### produces at this width,
// is stored as that character followed by U+0001, as each such cell would otherwise take 255
// characters. Runs of digits are kept, so a miscount in a very long number still shows.
import { spawnSync } from 'node:child_process';
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { crc32, deflateRawSync } from 'node:zlib';
import { root } from './lib.mjs';

const outDir = join(root, 'tests/workbooks/generated/format-zoo');
const dataDir = join(root, 'crates/cgs-numfmt/tests/data');

const perWorkbook = 180;
const columnWidth = 255;

const err = (text) => ({ error: text });

// Applied to every code. Together these reach each section, rounding edge and date range.
const coreValues = [
  0,
  1,
  5,
  12,
  123,
  1234,
  12345,
  1234567,
  12345678901,
  123456789012,
  1234567890123456,
  1e15,
  1e21,
  1.23456789e20,
  1e100,
  -1,
  -5,
  -1234,
  -1234567,
  -1e15,
  0.5,
  0.25,
  0.1,
  0.125,
  1.5,
  2.5,
  0.333333333333333,
  3.14159265358979,
  1234.5678,
  0.999,
  1.005,
  99.5,
  9999.5,
  -0.5,
  -1.5,
  -3.14159265358979,
  -1234.5678,
  0.4,
  0.004,
  0.0004,
  -0.4,
  -0.004,
  -0.0004,
  0.00012345678,
  1.23456789e-5,
  1e-10,
  1e-100,
  -1e-10,
  1.7976931348623157e308,
  2,
  59,
  60,
  61,
  366,
  21916,
  43831,
  45292,
  45352.5,
  45000.999999,
  2958465,
  2958465.99999,
  2958466,
  0.75,
  1.75,
  0.0000115740740740741,
  -45292,
  'abc',
  '123',
  'Hello World',
  ' sp ',
  true,
  false,
  err('#N/A'),
  err('#DIV/0!'),
];

// Only for the codes where these values say something more.
const generalValues = [
  10,
  100,
  1000000,
  1e9,
  1e10,
  1e11,
  1e12,
  1e14,
  1e20,
  1e99,
  1.5e100,
  1.234567890123e100,
  99999999999,
  99999999999.5,
  9999999999.5,
  999999999.5,
  12345678901.5,
  1234567890.5,
  123456789.5,
  12345.678901234,
  1234567.891,
  0.1234567890123,
  0.123456789,
  0.12345678951,
  0.9999999999,
  0.99999999999,
  1.9999999999,
  1.99999999999,
  1.5e11,
  1.2e15,
  1.2e20,
  -12345678901,
  -123456789012,
  -0.000123456789,
  -1.23456789e-5,
  1e-1,
  1e-2,
  1e-3,
  1e-4,
  1e-5,
  1e-6,
  1e-7,
  1e-8,
  1e-9,
  1e-11,
  1.5e-9,
  1.5e-10,
  1.2e-5,
  1.23e-5,
  1.234e-5,
  1.2345e-5,
  1.23456e-5,
  1.234567e-5,
  1.2e-6,
  1.23e-6,
  1.234e-6,
  1.2e-7,
  1.23e-7,
  1.2e-8,
  0.00001234,
  0.000123,
  0.00123456789,
  0.0123456789,
  1.23456e-20,
  1.23456e-99,
  1.23456e-100,
  1.23456e100,
  1234567890123,
  0.30000000000000004,
  0.1 + 0.7,
  1e15 + 0.5,
];

const numberValues = [
  0.05,
  0.15,
  0.35,
  0.45,
  0.55,
  0.95,
  3.5,
  1.115,
  1.125,
  1.995,
  1.0049999999,
  1.00499999999999,
  1.004999999999999,
  1.0049999999999999,
  0.045,
  0.285,
  1.45,
  8.325,
  1000000.5,
  2.675,
  1.1,
  1.2,
  1.3,
  1.4,
  1.6,
  1.7,
  1.8,
  1.9,
  0.2,
  0.3,
  0.6,
  0.7,
  0.8,
  0.9,
  0.99,
  0.9999,
  0.99999,
  3.999,
  12.0001,
  123.456,
  1 / 3,
  2 / 3,
  1 / 7,
  22 / 7,
  355 / 113,
  0.0625,
  0.1875,
  5.0625,
  100.5,
  -0.1,
  -0.125,
  -2.5,
  -1 / 3,
  -123.456,
  -0.999,
  -99.5,
  1e5,
  1e6,
  1e7,
  999999,
  999999.5,
  1000,
  999,
  1500,
  2500,
  1500000,
  2500000,
  500,
  499,
  1e-1,
  1e-2,
  1e-3,
  1.5e-3,
  1e-5,
  1e-20,
  1.5e20,
  9.99e20,
  9.5e20,
  1e22,
  1e30,
  1e50,
  1.5e50,
];

// Whole days across the Hijri cycle, times around each rounding edge, and the first days of 1900.
const dateValues = [
  0, 0.5, 3, 4, 31, 32, 58, 62, 63, 100, 365, 367, 1000, 1461, 1462, 3000, 5000, 7000, 9000, 11000,
  13000, 15000, 17000, 19000, 20000, 22000, 24000, 26000, 28000, 30000, 32000, 34000, 36000, 36526,
  38000, 40000, 42000, 43000, 44000, 44562, 44927, 45000, 45100, 45200, 45300, 45400, 45500, 45600,
  45700, 45800, 45900, 46000, 46023, 46100, 47000, 48000, 50000, 55000, 60000, 70000, 80000, 100000,
  200000, 500000, 1000000, 2000000, 45293, 45294, 45295, 45296, 45297, 45298, 45299, 45320, 45351,
  45352, 45380, 45410, 45440, 45470, 45530, 45560, 45590, 45620, 45650, 45652, 45653, 45654, 45655,
  0.04166666666666666, 0.0006944444444, 1.0416666667, 0.999988, 0.99999, 0.999994, 0.999995,
  0.999999, 0.9999999, 0.99999999, 0.999999999, 0.9999999999, 0.5000057, 0.50000578, 0.50000579,
  0.5000115, 0.49999999, 45000.5, 45000.123456, 45000.123456789, 45000.9999, 45000.99999,
  45000.9999999, 45000.99999999, 45000.999999999, 45000.9999999999, 45000.00001, 45000.000001,
  45000.0000001, 1.5, 100.25, 1.0000001, 0.0000001, 0.00000001, 0.0000005, 0.0000057, 0.00000578,
  0.00001, 0.000011, 0.000012, 0.016, 0.017, 0.0416, 0.0417, 0.1, 0.11, 0.123456789, -0.75, -1.5,
  -0.0000115740740740741, -100.25, 10, 100.5, 1000.5, 10000.5, 2958464.5, 2958465.5, 2958465.999,
  2958465.9999, 2958465.999999, 2958465.9999999, 2958465.99999999, 2958465.999999999, 2958470,
  3000000, 1e7, 1e10, 1e15,
];

const sets = {
  one: [0],
  core: coreValues,
  general: generalValues,
  number: numberValues,
  date: dateValues,
};

let localColours = new Map();

const forTextFunction = (code) =>
  code
    .replace(/\[(black|blue|cyan|green|magenta|red|white|yellow)\]/gi, (all, name) => {
      const local = localColours.get(name.toLowerCase());
      return local ? `[${local}]` : all;
    })
    .replace(/\[color ?(\d+)\]/gi, (all, number) => {
      const local = localColours.get('color');
      return local ? `[${local}${number}]` : all;
    });

const fnValues = [1234.5678, -1234.5678, 0, 0.5, 45292.75, 'abc', true];

// These go through the extra sets as well as the core one.
const richCodes = {
  general: [
    'General',
    '"a"General"b"',
    'General;-General;"z"',
    '[$-2000401]General',
    '[$-409]General',
    '0.00E+00',
    '##0.0E+0',
    '0E+0',
    '###.##E+00',
    '#E+0',
    '#,##0',
    '0',
  ],
  number: [
    '0',
    '#',
    '?',
    '0.0',
    '0.00',
    '0.000',
    '#.##',
    '?.??',
    '#,##0',
    '#,##0.00',
    '0,',
    '0.0,',
    '0,,',
    '#,##0.0,,',
    '0%',
    '0.0%',
    '0.00%',
    '0.00E+00',
    '##0.0E+0',
    '0E+0',
    '###.##E+00',
    '# ?/?',
    '# ??/??',
    '# ???/???',
    '?/?',
    '??/??',
    '0/0',
    '# ?/2',
    '# ?/4',
    '# ?/8',
    '# ??/16',
    '# ?/10',
    '# ??/100',
    '#/#',
    '0 0/0',
    '# #/#',
    '# ?/3',
    '#,##0.00_);(#,##0.00)',
    '_(* #,##0.00_);_(* (#,##0.00);_(* "-"??_);_(@_)',
  ],
  date: [
    'yyyy-mm-dd',
    'yyyy-mm-dd hh:mm:ss',
    'yyyy-mm-dd hh:mm:ss.000',
    'yyyy-mm-dd hh:mm:ss.00',
    'yyyy-mm-dd hh:mm:ss.0',
    'yyyy-mm-dd hh:mm',
    'yyyy-mm-dd hh',
    'ddd d mmm yyyy',
    'mmmmm',
    'h:mm:ss AM/PM',
    'h AM/PM',
    'mm:ss',
    'mm:ss.0',
    'ss',
    'ss.000',
    's.00',
    '[h]',
    '[h]:mm',
    '[h]:mm:ss',
    '[h]:mm:ss.000',
    '[m]',
    '[m]:ss',
    '[m]:ss.00',
    '[s]',
    '[s].0',
    '[s].000',
    'b2yyyy/mm/dd',
    'b2yyyy-mm-dd hh:mm:ss',
    'b2dddd d mmmm yyyy',
    '[$-60401]dddd d mmmm yyyy',
    '[$-60401]yyyy/mm/dd',
    '[$-E0401]dddd d mmmm yyyy',
    '[$-E0401]yyyy/mm/dd',
    '[$-A0401]dddd d mmmm yyyy',
    '[$-401]dddd d mmmm yyyy',
    '[$-2000401]dddd d mmmm yyyy',
    'bbbb/mm/dd',
    '[$-411]ggge"/"m"/"d',
    'e/mm/dd',
  ],
};

// Hard cases that the public files do not cover, including codes Excel is expected to refuse.
const handCodes = String.raw`
#
?
0.0
#.#
?.?
.0
.#
.?
0.
#.
?.
00.00
##.##
??.??
0.0?
0.#?0
0.##
#.00
#.##
?.??
#?0
0?#
0.0?#
#0#
?0?
000
0000000000
0,0
#,#
?,?
0,000
#,###
?,???
?,??0
#,##0
#,##0.00
#,##0,
#,##0,,
#,##0,,,
#,##0.00,
#,##0.0,,
0,
0,,
0,,,
0.0,
0.0,,
0,.0
0,,.0
0.0,0
0.,0
#,
#,,
,0
,#
,
0,0,0
0,#
#,0,
0 0
0-0
0 0 0
000-00-0000
(000) 000-0000
00\ 00\ 00
#\ ##0
# ##0
#\ ##0.0
#-#
#"x"#
0"x"0"y"0
0.0"x"0
0.#"x"#
#.#"x"#
0%
0%%
%0
%
%%
0.0%
#%
?%
0%,
0,%
0.00%;-0.00%
0%0
0%"x"
"x"0%
0E+0
0E-0
0.0E+0
0.00E-00
0.00E+000
00E+00
#E+0
?E+0
##0.0E+0
###.##E+00
##E+0
###E+0
####E+0
0.###E+0
#.#E+0
#.0E+0
#,##0E+0
0,E+0
0.0E+0,
0E+00%
0%E+00
0.00e+00
0.00e-00
0E+0"x"
"x"0E+0
0E+0_)
0E+0;-0E+0
0E+0.0
0E+#
0E+?
0E+0E+0
E+0
0E
0E+
e+0
# ?/?
# ??/??
# ???/???
?/?
??/??
0/0
00/00
#/#
# #/#
# 0/0
0 0/0
0 ?/?
00 ?/?
# ?/2
# ?/3
# ?/4
# ?/8
# ?/10
# ??/16
# ??/100
# ?/1
?/16
# ??/?
# ?/??
# ?/0
# ?/5
# ?/25
# ???/1000
#,##0 ?/?
#" "?/?
"x"# ?/?"y"
# ?/?"y"
# ?/?;-# ?/?
# ?/?%
# ?/?_)
0.0/0
0.0 ?/?
# ?/?.0
#/
/#
0/
/0
0 /0
0/ 0
# ? /?
# ?/ ?
# ?"x"/?
# ?/"x"?
"abc"
"a"0"b"
\a0
0\a
\0
"0"
"#"
"@"
""
""0""
$0
-0
+0
(0)
0:0
0!
^0
&0
'0
~0
{0}
<0>
=0
0/
/0
_x0
0_x
_(0_)
_ 0
_-0_-
*-0
0*-
*x0
0*x
* 0
0*
0*
"a"*-"b"
"a"*-"b"0
0"a"*-"b"*=0
*-*=0
*\0
*"0
€0
£0
¥0
"€"0
\€0
[Red]0
[Black]0
[Blue]0
[Cyan]0
[Green]0
[Magenta]0
[White]0
[Yellow]0
[Color1]0
[Color10]0
[Color 10]0
[Color56]0
[Color57]0
[Color0]0
[Color]0
[red]0
[RED]0
[Blue]0;[Red]0;[Green]0;[Yellow]@
[Red]@
[Red][>1]0
[>1][Red]0
[Red][Blue]0
[Orange]0
[>100]0
[<=0]0
[>5]0
[<5]0
[=5]0
[<>5]0
[>=5]0
[<=5]0
[>100]"big";[<=0]"neg";"mid"
[>100]0;0
[>100]0;[<0]0
[>100]0;[<0]0;
[=1]"one";0
[<>1]"not one";0
[>=1]0;[<1]0.00
[<0]0;0
[<0]"n"0;0
[>0]0;0
[>0]0;0;0
[>0]0;[<-1]0;"z"
[<0]0;[>0]0;"z"0
[=0]"z";0
[>=0]0;-0
[>-5]0;0
[>-5]0
[<-5]0
[>1.5]0.0;0
[>1E3]0;0
[>1e3]0;0
[>100]0;[>10]0.0;0.00
[<100]0;[<1000]0.0;0.00
[>100]0;[<=0]0;@
[>100]0;0;0;@
0;[>100]0
0;0;[>100]0
0;[<0]0
[<1]0.00;[<10]0.0;0
[Red][<0]0;0
[>100]0;;
[>100];0
[>0];[<0];
[>100]0;[>10]0;[>1]0
[>100]0;[>10]0;[>1]0;@
[>]0
[100]0
[>a]0
[> 100]0
[>100 ]0
[>=-1]0;0
[<=-1]0;0
[<=-1]-0;0
[>=0]0;[<0]-0
[>=0]0;[<0]0
[<0]-0;[>=0]0
[<0]0;[>=0]0
[=-5]0;0
[=-5]-0;0
[<0]0
[<0]-0
[<0]\(0\);0.00
0;[Red]0
0;[Red]-0
0;(0)
0;-0
0;0;0
0;-0;"z"
0;-0;;
0;0
0;
0;;
;
;;
;;;
;0
;;0
;;;@
;;;;
0;0;0;0;0
"p";"n";"z";"t"
0;0;0;
0;0;0;"text"
@
@@
@ @
"a"@"b"
@" "@
\a@
@\a
_(@_)
*-@
@*-
0;@
0;0;@
0;0;0;@
#;@
@;@
@;@;@
@;@;@;@
@;0
@;0;0
@;0;0;0
0;@;0
0;0;@;0
General;@
General;General
General;-General;"z"
General;[Red]General
General;General;General;General
"x"General
General"x"
General_)
_(General_)
*-General
General*-
[Red]General
[>100]General;0
[>100]0;General
[<0]General
General;0
0;General
GENERAL
general
GeNeRaL
General%
%General
General 0
0 General
General General
General@
@General
Generalx
y
yy
yyy
yyyy
yyyyy
Y
YY
YYYY
m
mm
mmm
mmmm
mmmmm
mmmmmm
M
MM
MMM
MMMM
MMMMM
d
dd
ddd
dddd
ddddd
dddddd
D
DD
DDD
DDDD
h
hh
hhh
H
HH
s
ss
sss
S
SS
AM/PM
A/P
a/p
am/pm
Am/Pm
A/p
a/P
h AM/PM
hh AM/PM
h A/P
hh a/p
h am/pm
h A/p
AM/PM h
A/P h
a/p h
h:m
h:mm
hh:mm
h:m:s
m:s
mm:ss
m:ss
mm:s
h m s
h mm ss
h"x"m
h\m
h m
hh m
h d m
h yy m
h ss m
h AM/PM m
h AM/PM mm
s m
ss mm
mm ss
mm h
m h
m d s
m "x" s
m\xs
m y s
d h m
y m d h m s
yyyy-mm-dd hh:mm:ss
m/d/yy h:mm
h:mm m
m h:mm m s
m m
mm mm
m s m
ss.0
ss.00
ss.000
ss.0000
s.0
s.000
mm:ss.0
h:mm:ss.00
h:mm:ss.000 AM/PM
ss.0 AM/PM
ss.#
ss.?
ss.0#
.0
h.0
m.0
yyyy.0
[h]
[hh]
[hhh]
[H]
[m]
[mm]
[mmm]
[M]
[s]
[ss]
[sss]
[S]
[h]:mm
[h]:mm:ss
[h]:mm:ss.000
[m]:ss
[mm]:ss.0
[s].00
[ss].0
[h] AM/PM
[h]:m
[m]:s
h:[m]
h:[mm]:ss
d [h]
yyyy [h]
[h]:[m]
[m]:[s]
[h] h
[h] m
[m] m
[m] h
[s] s
[h]:mm;@
[h]:mm;[h]:mm
[h];-[h]
-[h]
"x"[h]
[Red][h]
[>1][h];h
"x"yyyy"y"
yyyy\-mm
yyyy;@
yyyy;yyyy;yyyy;@
0;yyyy
yyyy;0
mmm-yy;0
0.00;mmm-yy
d/m;[Red]d/m;"z"
[Red]yyyy
[>45000]yyyy;0
[<1]h:mm;d h:mm
[<1]h:mm;[h]:mm
[>=1]d;h
_(yyyy_)
*-yyyy
yyyy*-
yyyy 0
0 yyyy
yyyy #
yyyy ?
yyyy%
yyyy,
yyyy E+0
yyyy@
@yyyy
General yyyy
yyyy General
HH:MM:SS
hh:MM
MM:ss
DD/MM/YYYY
d/m/y
\d0
"d"0
b
bb
bbb
bbbb
B
BB
e
ee
eee
E
EE
g
gg
ggg
gggg
G
b1
b2
B1
B2
b1yyyy
b2yyyy
B2yyyy/mm/dd
b1dd/mm/yyyy
b3yyyy
b2e
b2bb
b2mmmm
b2mmm
b2mmmmm
b2dddd
b2ddd
b2hh:mm
yyyy b2
0b2
b20
b2General
b2@
b2"x"
\a
\b
a
c
f
i
j
k
l
n
o
p
q
r
t
u
v
w
x
z
A
C
F
I
J
K
L
N
O
P
Q
R
T
U
V
W
X
Z
0a
0b
0c
0d
0e
0f
0g
0h
0i
0j
0k
0l
0m
0n
0o
0p
0q
0r
0s
0t
0u
0v
0w
0x
0y
0z
0 kg
d a
d b
d c
d f
d n
d x
d z
#x
?x
h x
abc
"unterminated
\
_
*
0\
0_
0*
[Red
Red]0
[Foo]0
[$
[$-
[$]
[]0
[>1][>2][>3]0
[>1][<2]0
yyyy/
h:mm:ss.0000
0.0.0
0..0
..
.
0;0;0;0;
"a"0;"b"0;"c"0;"d"@;"e"
[$-409]0
[$-409]dddd d mmmm yyyy
[$-409]h:mm:ss AM/PM
[$-401]0
[$-401]#,##0.00
[$-401]0%
[$-401]General
[$-401]@
[$-401]dddd d mmmm yyyy
[$-401]ddd d mmm yy
[$-401]mmmmm
[$-401]yyyy/mm/dd
[$-401]h:mm:ss AM/PM
[$-401]hh:mm A/P
[$-401]e
[$-401]b
[$-401]g
[$-401]gg
[$-401]ggg
[$-801]dddd d mmmm yyyy
[$-801]ddd d mmm yy
[$-801]h:mm AM/PM
[$-C01]dddd d mmmm yyyy
[$-C01]ddd d mmm yy
[$-C01]h:mm:ss AM/PM
[$-1001]dddd d mmmm yyyy
[$-1401]dddd d mmmm yyyy
[$-1801]dddd d mmmm yyyy
[$-1C01]dddd d mmmm yyyy
[$-2001]dddd d mmmm yyyy
[$-2401]dddd d mmmm yyyy
[$-2801]dddd d mmmm yyyy
[$-2C01]dddd d mmmm yyyy
[$-3001]dddd d mmmm yyyy
[$-3401]dddd d mmmm yyyy
[$-3801]dddd d mmmm yyyy
[$-3C01]dddd d mmmm yyyy
[$-4001]dddd d mmmm yyyy
[$-40C]dddd d mmmm yyyy
[$-40C]ddd d mmm yy
[$-40C]h:mm AM/PM
[$-40C]0.00
[$-407]dddd d mmmm yyyy
[$-407]ddd d mmm yy
[$-407]#,##0.00
[$-407]h:mm AM/PM
[$-40A]dddd d mmmm yyyy
[$-40A]ddd d mmm yy
[$-C0A]dddd d mmmm yyyy
[$-C0A]ddd d mmm yy
[$-C0A]h:mm AM/PM
[$-410]dddd d mmmm yyyy
[$-416]dddd d mmmm yyyy
[$-419]dddd d mmmm yyyy
[$-41F]dddd d mmmm yyyy
[$-411]dddd d mmmm yyyy
[$-411]e
[$-411]ee
[$-411]g
[$-411]gg
[$-411]ggg
[$-411]ggge/m/d
[$-411]gggee/m/d
[$-411]yyyy/m/d
[$-411]0
[$-412]dddd d mmmm yyyy
[$-804]dddd d mmmm yyyy
[$-404]dddd d mmmm yyyy
[$-F800]0
[$-F800]yyyy
[$-F800]dddd, mmmm d, yyyy
[$-F800]dddd\,\ mmmm\ dd\,\ yyyy
[$-F400]0
[$-F400]yyyy
[$-F400]h:mm:ss AM/PM
[$-F400]h:mm
[$-x-sysdate]dddd, mmmm dd, yyyy
[$-x-systime]h:mm:ss AM/PM
[$-en-US]dddd d mmmm yyyy
[$-ar-SA]dddd d mmmm yyyy
[$-fr-FR]dddd d mmmm yyyy
[$-de-DE]dddd d mmmm yyyy
[$-es-ES]dddd d mmmm yyyy
[$-ar-SA,1]dddd d mmmm yyyy
[$€-407]#,##0.00
[$€-407] #,##0.00
#,##0.00 [$€-407]
[$$-409]#,##0
[$$-409]0.00;[Red][$$-409]0.00
[$£-809]0.00
[$¥-411]0
[$€]0
[$]0
[$-]0
[$USD]0
[$USD-409]0
[$AED-3801]0.00
[$د.إ.‏-3801]0.00
[$ريال-401]0
[$ر.س.‏-401]#,##0.00
#,##0.00\ [$€-1]
[$€-1]0;[Red][$€-1]-0
[$€-407]yyyy
[$€-407]@
[$-407]@
[$x-407]General
[$"x"-409]0
[$\x-409]0
[$-1]0
[$-0]0
[$-00]0
[$-0409]0
[$-00000409]dddd
[$-1000000]0
[$-2000000]0
[$-3000000]0
[$-4000000]0
[$-5000000]0
[$-10000000]0
[$-1000409]0
[$-2000409]0
[$-3000409]0
[$-2000401]0
[$-3000401]0
[$-1000401]0
[$-0000401]0
[$-2000401]0.00
[$-3000401]0.00
[$-2000401]#,##0
[$-2000401]#,##0.00
[$-3000401]#,##0.00
[$-2000401]0%
[$-2000401]0.00E+00
[$-2000401]# ?/?
[$-2000401]General
[$-3000401]General
[$-1000401]General
[$-2000401]@
[$-2000401]yyyy/mm/dd
[$-3000401]yyyy/mm/dd
[$-1000401]yyyy/mm/dd
[$-2000401]h:mm:ss AM/PM
[$-3000401]h:mm:ss AM/PM
[$-2000401]dddd d mmmm yyyy
[$-3000401]dddd d mmmm yyyy
[$-2000401][h]:mm
[$-2000401]"x"0
[$-2000401]"1"0
[$-2000401]\10
[$-2000401]-0
[$-2000401](0)
[$-2000401]0;(0)
[$-10401]dddd d mmmm yyyy
[$-10401]yyyy/mm/dd
[$-20401]dddd d mmmm yyyy
[$-20401]ddd d mmm yy
[$-20401]yyyy/mm/dd
[$-20401]h:mm AM/PM
[$-60401]dddd d mmmm yyyy
[$-60401]ddd d mmm yy
[$-60401]yyyy/mm/dd
[$-60401]yy/m/d
[$-60401]e/m/d
[$-60401]h:mm AM/PM
[$-60401]yyyy/mm/dd hh:mm:ss
[$-A0401]dddd d mmmm yyyy
[$-A0401]ddd d mmm yy
[$-A0401]yyyy/mm/dd
[$-A0401]h:mm AM/PM
[$-E0401]dddd d mmmm yyyy
[$-E0401]ddd d mmm yy
[$-E0401]yyyy/mm/dd
[$-E0401]e/m/d
[$-E0401]h:mm AM/PM
[$-1000401]dddd d mmmm yyyy
[$-1010401]dddd d mmmm yyyy
[$-1020401]dddd d mmmm yyyy
[$-1060401]dddd d mmmm yyyy
[$-1060401]yyyy/mm/dd
[$-10A0401]dddd d mmmm yyyy
[$-10E0401]dddd d mmmm yyyy
[$-10E0401]yyyy/mm/dd
[$-2010401]dddd d mmmm yyyy
[$-2020401]dddd d mmmm yyyy
[$-2060401]dddd d mmmm yyyy
[$-2060401]yyyy/mm/dd
[$-3060401]dddd d mmmm yyyy
[$-3060401]yyyy/mm/dd
[$-20A0401]dddd d mmmm yyyy
[$-20A0401]yyyy/mm/dd
[$-20E0401]dddd d mmmm yyyy
[$-20E0401]yyyy/mm/dd
[$-30E0401]yyyy/mm/dd
[$-60409]dddd d mmmm yyyy
[$-60409]yyyy/mm/dd
[$-60000]dddd d mmmm yyyy
[$-60000]yyyy/mm/dd
[$-A0000]dddd d mmmm yyyy
[$-A0409]dddd d mmmm yyyy
[$-E0000]dddd d mmmm yyyy
[$-E0409]dddd d mmmm yyyy
[$-E0409]yyyy/mm/dd
[$-1000000]dddd
[$-30401]dddd d mmmm yyyy
[$-70401]dddd d mmmm yyyy
[$-70401]yyyy/mm/dd
[$-80401]yyyy/mm/dd
[$-B0401]dddd d mmmm yyyy
[$-C01]b2dddd d mmmm yyyy
[$-401]b2dddd d mmmm yyyy
[$-401]b2yyyy/mm/dd
[$-401]b1dddd d mmmm yyyy
[$-401]b1yyyy/mm/dd
[$-409]b2dddd d mmmm yyyy
[$-409]b2yyyy/mm/dd
[$-60401]b1yyyy/mm/dd
[$-60401]b1dddd d mmmm yyyy
[$-A0401]b2yyyy/mm/dd
b2[$-401]yyyy/mm/dd
b2[$-401]dddd d mmmm yyyy
[$-40C]b2dddd d mmmm yyyy
[$-407]b2dddd d mmmm yyyy
[$-170401]dddd d mmmm yyyy
[$-170401]yyyy/mm/dd
[$-170409]dddd d mmmm yyyy
[$-170409]yyyy/mm/dd
1
01
10
0 1
1 0
0.1
0.01
1.0
1%
1E+0
1/2
#1
"x"1
1"x"
@1
1@
yyyy1
1yyyy
[DBNum1]0
[DBNum2]0
[DBNum3]0
[DBNum4]0
[DBNum1][$-411]General
[$-401]dddd;[$-409]dddd
[$-409]dddd;[$-401]dddd
[$-401]yyyy;@
[$-401]yyyy;[$-401]@
[$-2000401]0;[$-3000401]0
`
  .split('\n')
  .filter((line) => line.length > 0);

// A line in the template above can't show its own trailing space clearly, so these are spelled out.
handCodes.push('0 ', ' 0', ' ', '  ', '0_ ', '_ ', '"a" ', ' "a"', '@ ', ' @', 'yyyy ', ' yyyy');

// Date codes run in a date1904 workbook too.
const codes1904 = [
  'General',
  '0',
  'yyyy-mm-dd',
  'yyyy-mm-dd hh:mm:ss',
  'yyyy-mm-dd hh:mm:ss.000',
  'ddd d mmm yyyy',
  'dddd',
  'mmmm',
  'h:mm:ss AM/PM',
  '[h]:mm:ss',
  '[m]',
  '[s]',
  'b2yyyy/mm/dd',
  '[$-60401]dddd d mmmm yyyy',
  '[$-401]dddd d mmmm yyyy',
  '[$-411]ggge/m/d',
  'bbbb',
  'e',
];

const builtinIds = Array.from({ length: 82 }, (_, id) => id);

const escapeXml = (text) =>
  text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

const columnName = (index) => {
  let text = '';
  for (let rest = index; rest > 0; rest = Math.floor((rest - 1) / 26)) {
    text = String.fromCharCode(65 + ((rest - 1) % 26)) + text;
  }
  return text;
};

function zip(entries) {
  const chunks = [];
  const central = [];
  let offset = 0;
  for (const [name, text] of entries) {
    const body = Buffer.from(text, 'utf8');
    const packed = deflateRawSync(body, { level: 6 });
    const nameBytes = Buffer.from(name);
    const crc = crc32(body);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt16LE(8, 8);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(packed.length, 18);
    local.writeUInt32LE(body.length, 22);
    local.writeUInt16LE(nameBytes.length, 26);
    const header = Buffer.alloc(46);
    header.writeUInt32LE(0x02014b50, 0);
    header.writeUInt16LE(20, 4);
    header.writeUInt16LE(20, 6);
    header.writeUInt16LE(8, 10);
    header.writeUInt32LE(crc, 16);
    header.writeUInt32LE(packed.length, 20);
    header.writeUInt32LE(body.length, 24);
    header.writeUInt16LE(nameBytes.length, 28);
    header.writeUInt32LE(offset, 42);
    central.push(header, nameBytes);
    chunks.push(local, nameBytes, packed);
    offset += local.length + nameBytes.length + packed.length;
  }
  const directory = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(directory.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...chunks, directory, end]);
}

function valueCell(at, style, value) {
  const s = style === null ? '' : ` s="${style}"`;
  if (typeof value === 'number') return `<c r="${at}"${s}><v>${value}</v></c>`;
  if (typeof value === 'boolean') return `<c r="${at}"${s} t="b"><v>${value ? 1 : 0}</v></c>`;
  if (typeof value === 'object') return `<c r="${at}"${s} t="e"><v>${value.error}</v></c>`;
  return `<c r="${at}"${s} t="inlineStr"><is><t xml:space="preserve">${escapeXml(value)}</t></is></c>`;
}

const fnLiteral = (value) =>
  typeof value === 'string'
    ? `"${value}"`
    : typeof value === 'boolean'
      ? String(value).toUpperCase()
      : String(value);

const header =
  '<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">' +
  `<cols><col min="1" max="1024" width="${columnWidth}.7109375" customWidth="1"/></cols><sheetData>`;

/** Column A of a row counts the cells that follow it, which is all the reader needs to know. */
function workbookZip(rows, { date1904 = false } = {}) {
  const formats = new Map();
  const xfs = [];
  const style = (row) => {
    if (row.builtin !== undefined) {
      xfs.push(row.builtin);
    } else {
      if (!formats.has(row.code)) formats.set(row.code, 164 + formats.size);
      xfs.push(formats.get(row.code));
    }
    return xfs.length; // Style 0 is the default.
  };
  const cells = ['<row r="1">'];
  const fns = [];
  rows.forEach((row, index) => {
    const r = index + 1;
    const s = row.invalid ? null : style(row);
    const values = row.invalid ? [] : sets[row.set];
    const line = [`<row r="${r}"><c r="A${r}"><v>${values.length}</v></c>`];
    values.forEach((value, at) => line.push(valueCell(columnName(at + 2) + r, s, value)));
    line.push('</row>');
    cells.push(line.join(''));
    const fnLine = [`<row r="${r}"><c r="A${r}"><v>${row.fn ? fnValues.length : 0}</v></c>`];
    if (row.fn) {
      fnValues.forEach((value, at) => {
        const code = forTextFunction(row.code).replace(/"/g, '""');
        fnLine.push(
          `<c r="${columnName(at + 2) + r}" t="str"><f>TEXT(${escapeXml(fnLiteral(value))},${escapeXml(`"${code}"`)})</f></c>`,
        );
      });
    }
    fnLine.push('</row>');
    fns.push(fnLine.join(''));
  });
  cells.shift();
  const numFmts = [...formats]
    .map(([code, id]) => `<numFmt numFmtId="${id}" formatCode="${escapeXml(code)}"/>`)
    .join('');
  const styles =
    '<?xml version="1.0" encoding="UTF-8"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">' +
    (formats.size ? `<numFmts count="${formats.size}">${numFmts}</numFmts>` : '') +
    '<fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts>' +
    '<fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills>' +
    '<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>' +
    '<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>' +
    `<cellXfs count="${xfs.length + 1}"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>` +
    xfs
      .map(
        (id) =>
          `<xf numFmtId="${id}" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>`,
      )
      .join('') +
    '</cellXfs></styleSheet>';
  return zip([
    [
      '[Content_Types].xml',
      '<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>',
    ],
    [
      '_rels/.rels',
      '<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>',
    ],
    [
      'xl/workbook.xml',
      `<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><workbookPr${date1904 ? ' date1904="1"' : ''}/><sheets><sheet name="cells" sheetId="1" r:id="rId1"/><sheet name="fn" sheetId="2" r:id="rId2"/></sheets><calcPr fullCalcOnLoad="1"/></workbook>`,
    ],
    [
      'xl/_rels/workbook.xml.rels',
      '<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>',
    ],
    ['xl/styles.xml', styles],
    ['xl/worksheets/sheet1.xml', header + cells.join('') + '</sheetData></worksheet>'],
    ['xl/worksheets/sheet2.xml', header + fns.join('') + '</sheetData></worksheet>'],
  ]);
}

function powershell(script) {
  const result = spawnSync('powershell', ['-NoProfile', '-NonInteractive', '-Command', script], {
    encoding: 'utf8',
    timeout: 3_600_000,
    maxBuffer: 1 << 28,
  });
  if (result.status !== 0) throw new Error('PowerShell failed: ' + result.stderr);
  return result.stdout;
}

/** One Excel instance with the same safety settings as generate-test-workbooks.mjs. */
const withExcel = (body) => `
$ErrorActionPreference = 'Stop'
$x = New-Object -ComObject Excel.Application
try {
  $x.Visible = $false
  $x.DisplayAlerts = $false
  $x.AskToUpdateLinks = $false
  $x.AutomationSecurity = 3
${body}
} finally {
  $x.Quit()
  [void][Runtime.InteropServices.Marshal]::ReleaseComObject($x)
}`;

const escapeLine = `function Esc($t) { return ([string]$t).Replace('\\', '\\\\').Replace([string][char]9, '\\t').Replace([string][char]10, '\\n').Replace([string][char]13, '\\r') }`;

const unescapeLine = (text) =>
  text.replace(/\\(.)/g, (_, c) => (c === 't' ? '\t' : c === 'n' ? '\n' : c === 'r' ? '\r' : c));

/** Which of these workbooks Excel will open. A code it can't read makes Open fail. */
function canOpen(paths) {
  const output = join(outDir, 'open.txt');
  powershell(
    withExcel(`
  $out = New-Object IO.StreamWriter('${output}', $false, (New-Object Text.UTF8Encoding($false)))
  foreach ($path in @(${paths.map((path) => `'${path}'`).join(', ')})) {
    try {
      $w = $x.Workbooks.Open($path, 0, $true)
      $w.Close($false)
      $out.WriteLine('1')
    } catch {
      $out.WriteLine('0')
    }
  }
  $out.Close()`),
  );
  return readFile(output, 'utf8').then((text) =>
    text
      .split('\n')
      .filter(Boolean)
      .map((line) => line[0] === '1'),
  );
}

/**
 * The codes Excel refuses when it reads them from a file, which is what matters here. Range.
 * NumberFormat is no use for this, because it expects colour names in Office's own language.
 * Whole workbooks are tried first, and each one that fails is split until single codes remain.
 */
async function refusedCodes(codes) {
  const refused = new Set();
  let pending = [];
  for (let at = 0; at < codes.length; at += perWorkbook) {
    pending.push(codes.slice(at, at + perWorkbook));
  }
  sets.preflight = [0];
  const probes = join(outDir, 'preflight');
  await mkdir(probes, { recursive: true });
  while (pending.length) {
    const paths = [];
    for (const [index, group] of pending.entries()) {
      const path = join(probes, `${index}.xlsx`);
      await writeFile(path, workbookZip(group.map((code) => ({ code, set: 'preflight' }))));
      paths.push(path);
    }
    // One command line holds only so much, so the workbooks go in batches.
    const answers = [];
    for (let at = 0; at < paths.length; at += 100) {
      answers.push(...(await canOpen(paths.slice(at, at + 100))));
    }
    const next = [];
    pending.forEach((group, index) => {
      if (answers[index]) return;
      if (group.length === 1) refused.add(group[0]);
      else {
        const half = group.length >> 1;
        next.push(group.slice(0, half), group.slice(half));
      }
    });
    pending = next;
  }
  delete sets.preflight;
  await rm(probes, { recursive: true, force: true });
  return refused;
}

/**
 * TEXT() takes colour names in Office's language, as Range.NumberFormat does, so it is given them
 * that way. Excel reports the names itself when it reads them from a file.
 */
async function colourNames() {
  const names = ['Black', 'Blue', 'Cyan', 'Green', 'Magenta', 'Red', 'White', 'Yellow', 'Color7'];
  const path = join(outDir, 'colours.xlsx');
  await writeFile(path, workbookZip(names.map((name) => ({ code: `[${name}]0`, set: 'one' }))));
  readWorkbooks([path]);
  const lines = (await readFile(path + '.txt', 'utf8'))
    .split('\n')
    .filter((line) => line.startsWith('cells'));
  const map = new Map();
  lines.forEach((line, index) => {
    const local = /^\[(.*)\]0$/.exec(unescapeLine(line.split('\t')[2]))[1];
    map.set(names[index].toLowerCase().replace(/\d+/, ''), local.replace(/\d+/, ''));
  });
  return map;
}

/** Reads each row's cells from both sheets of the given workbooks into text files beside them. */
function readWorkbooks(paths) {
  powershell(
    withExcel(
      `
  ${escapeLine}
` +
        paths
          .map(
            (path) => `
  $w = $x.Workbooks.Open('${path}', 0, $true)
  $x.CalculateFull()
  $out = New-Object IO.StreamWriter('${path}.txt', $false, (New-Object Text.UTF8Encoding($false)))
  foreach ($sheet in @('cells', 'fn')) {
    $s = $w.Worksheets.Item($sheet)
    $last = $s.UsedRange.Rows.Count
    for ($r = 1; $r -le $last; $r++) {
      $n = [int]$s.Cells.Item($r, 1).Value2
      $line = New-Object Text.StringBuilder
      [void]$line.Append($sheet).Append([char]9).Append($r).Append([char]9).Append((Esc $s.Cells.Item($r, 2).NumberFormat))
      for ($c = 2; $c -le $n + 1; $c++) {
        [void]$line.Append([char]9).Append((Esc $s.Cells.Item($r, $c).Text))
      }
      $out.WriteLine($line.ToString())
    }
  }
  $out.Close()
  $w.Close($false)`,
          )
          .join('\n'),
    ),
  );
}

const compress = (text) => text.replace(/(\P{N})\1{49,}/gu, (_, c) => c + '\u0001');

await rm(outDir, { recursive: true, force: true });
await mkdir(outDir, { recursive: true });

const harvested = (await readFile(join(dataDir, 'format-codes.txt'), 'utf8'))
  .split('\n')
  .filter((line) => line.length > 0);

const allCodes = [...new Set([...harvested, ...handCodes, ...codes1904])];
const refused = await refusedCodes(allCodes);
console.log(`${allCodes.length} codes, Excel refuses ${refused.size} of them`);
localColours = await colourNames();

const rows = [];
const harvestedSet = new Set(harvested);
const usesSpread = new Set([...richCodes.general, ...richCodes.number, ...richCodes.date]);
for (const code of new Set([...harvested, ...handCodes, ...usesSpread])) {
  const group = harvestedSet.has(code) ? 'harvested' : 'hand';
  const invalid = refused.has(code);
  rows.push({ group, code, set: 'core', invalid, fn: true });
}
for (const set of ['general', 'number', 'date']) {
  for (const code of richCodes[set])
    rows.push({ group: 'hand', code, set, invalid: false, fn: false });
}

const builtinRows = builtinIds.map((id) => ({
  group: 'builtin',
  builtin: id,
  code: '',
  set: 'core',
  invalid: false,
  fn: false,
}));

const rows1904 = codes1904.flatMap((code) =>
  ['core', 'date'].map((set) => ({
    group: '1904',
    code,
    set,
    invalid: false,
    fn: false,
    date1904: true,
  })),
);

const files = [];
const addWorkbook = async (name, chunk, options) => {
  const path = join(outDir, name + '.xlsx');
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, workbookZip(chunk, options));
  files.push({ path, chunk });
};
await addWorkbook('builtin', builtinRows);
await addWorkbook('date1904', rows1904, { date1904: true });
{
  // Rows of one code in several sets share a format, so only distinct codes count to the limit.
  let chunk = [];
  let distinct = new Set();
  let index = 0;
  for (const row of rows) {
    if (!row.invalid) distinct.add(row.code);
    chunk.push(row);
    if (distinct.size >= perWorkbook) {
      await addWorkbook(`codes-${++index}`, chunk);
      chunk = [];
      distinct = new Set();
    }
  }
  if (chunk.length) await addWorkbook(`codes-${index + 1}`, chunk);
}

const started = Date.now();
readWorkbooks(files.map((file) => file.path.replace(/\//g, '\\')));
console.log(`read ${files.length} workbooks in ${((Date.now() - started) / 1000).toFixed(0)} s`);

const records = [];
let cells = 0;
for (const { path, chunk } of files) {
  const lines = (await readFile(path.replace(/\//g, '\\') + '.txt', 'utf8')).split('\n');
  const found = { cells: new Map(), fn: new Map() };
  for (const line of lines) {
    if (!line) continue;
    const [sheet, r, stored, ...texts] = line.replace(/\r$/, '').split('\t');
    found[sheet].set(Number(r), {
      stored: unescapeLine(stored),
      texts: texts.map((text) => compress(unescapeLine(text))),
    });
  }
  chunk.forEach((row, index) => {
    const seen = found.cells.get(index + 1);
    const record = { group: row.group };
    if (row.builtin !== undefined) record.builtin = row.builtin;
    record.code = row.builtin !== undefined ? seen.stored : row.code;
    record.set = row.set;
    if (row.date1904) record.date1904 = true;
    if (row.invalid) record.invalid = true;
    else {
      record.text = seen.texts;
      cells += seen.texts.length;
      if (seen.texts.length !== sets[row.set].length) {
        throw new Error(`${path}: row ${index + 1} has ${seen.texts.length} texts`);
      }
    }
    if (row.fn) record.fn = found.fn.get(index + 1).texts;
    records.push(JSON.stringify(record));
  });
}

const head = JSON.stringify({
  note: 'See scripts/excel-format-zoo.mjs',
  columnWidth,
  sets,
  fn: fnValues,
});
const output = join(dataDir, 'excel-text.jsonl');
await writeFile(output, [head, ...records].join('\n') + '\n');
const size = (await readFile(output)).length;
console.log(
  `${records.length} records, ${cells} cells, ${(size / 1048576).toFixed(2)} MB written to ${output}`,
);
