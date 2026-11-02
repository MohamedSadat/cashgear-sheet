//! Regional settings, and the names a locale tag in a format code asks for.

/// What a format with no locale tag of its own follows. The fields are public so JS-supplied
/// `Intl` data can fill them in. Days start on Sunday.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locale {
    pub decimal: String,
    pub group: String,
    /// Format code for built-in 14 and its relatives.
    pub short_date: String,
    /// Format code for `[$-F800]`.
    pub long_date: String,
    pub short_time: String,
    /// Format code for `[$-F400]`.
    pub long_time: String,
    pub am: String,
    pub pm: String,
    pub months: [String; 12],
    pub months_short: [String; 12],
    pub days: [String; 7],
    pub days_short: [String; 7],
    pub currency: String,
    /// What `/` shows in a date format.
    pub date_separator: String,
    /// What `:` shows in a time format.
    pub time_separator: String,
}

impl Locale {
    pub fn en_us() -> Self {
        let owned = |names: &[&str]| {
            names
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>()
        };
        let twelve =
            |names: &[&str; 12]| -> [String; 12] { owned(names).try_into().unwrap_or_default() };
        let seven =
            |names: &[&str; 7]| -> [String; 7] { owned(names).try_into().unwrap_or_default() };
        Self {
            decimal: ".".to_string(),
            group: ",".to_string(),
            short_date: "m/d/yyyy".to_string(),
            long_date: "dddd, mmmm d, yyyy".to_string(),
            short_time: "h:mm AM/PM".to_string(),
            long_time: "h:mm:ss AM/PM".to_string(),
            am: ENGLISH.am.to_string(),
            pm: ENGLISH.pm.to_string(),
            months: twelve(&ENGLISH.months),
            months_short: twelve(&ENGLISH.months_short),
            days: seven(&ENGLISH.days),
            days_short: seven(&ENGLISH.days_short),
            currency: "$".to_string(),
            date_separator: "/".to_string(),
            time_separator: ":".to_string(),
        }
    }
}

impl Default for Locale {
    fn default() -> Self {
        Self::en_us()
    }
}

/// The names one language gives months and days.
#[derive(Debug)]
pub(crate) struct Names {
    pub months: [&'static str; 12],
    pub months_short: [&'static str; 12],
    pub days: [&'static str; 7],
    pub days_short: [&'static str; 7],
    pub am: &'static str,
    pub pm: &'static str,
}

pub(crate) static ENGLISH: Names = Names {
    months: [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ],
    months_short: [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ],
    days: [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ],
    days_short: ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"],
    am: "AM",
    pm: "PM",
};

const ARABIC_DAYS: [&str; 7] = [
    "الأحد",
    "الإثنين",
    "الثلاثاء",
    "الأربعاء",
    "الخميس",
    "الجمعة",
    "السبت",
];

const ARABIC_MONTHS: [&str; 12] = [
    "يناير",
    "فبراير",
    "مارس",
    "أبريل",
    "مايو",
    "يونيو",
    "يوليو",
    "أغسطس",
    "سبتمبر",
    "أكتوبر",
    "نوفمبر",
    "ديسمبر",
];

const LEVANT_MONTHS: [&str; 12] = [
    "كانون\u{a0}الثاني",
    "شباط",
    "آذار",
    "نيسان",
    "أيار",
    "حزيران",
    "تموز",
    "آب",
    "أيلول",
    "تشرين\u{a0}الأول",
    "تشرين\u{a0}الثاني",
    "كانون\u{a0}الأول",
];

const HIJRI_MONTHS: [&str; 12] = [
    "محرم",
    "صفر",
    "ربيع\u{a0}الأول",
    "ربيع\u{a0}الثاني",
    "جمادى\u{a0}الأولى",
    "جمادى\u{a0}الثانية",
    "رجب",
    "شعبان",
    "رمضان",
    "شوال",
    "ذو\u{a0}القعدة",
    "ذو\u{a0}الحجة",
];

const HIJRI_MONTHS_EGYPT: [&str; 12] = [
    "محرم",
    "صفر",
    "ربيع\u{a0}الأول",
    "ربيع\u{a0}الآخرة",
    "جمادى\u{a0}الأولى",
    "جمادى\u{a0}الآخرة",
    "رجب",
    "شعبان",
    "رمضان",
    "شوال",
    "ذو\u{a0}القعدة",
    "ذو\u{a0}الحجة",
];

const HIJRI_MONTHS_LATIN: [&str; 12] = [
    "Muharram",
    "Safar",
    "Rabi' Awwal",
    "Rabi' Thani",
    "Jamada El Oula",
    "Jamada El Thaniah",
    "Rajab",
    "Sha'ban",
    "Ramadan",
    "Shawwal",
    "Thoul Ki'dah",
    "Thoul Hijjah",
];

const AM_ARABIC: &str = "ص";
const PM_ARABIC: &str = "م";

pub(crate) static ARABIC: Names = Names {
    months: ARABIC_MONTHS,
    months_short: ARABIC_MONTHS,
    days: ARABIC_DAYS,
    days_short: ARABIC_DAYS,
    am: AM_ARABIC,
    pm: PM_ARABIC,
};

pub(crate) static ARABIC_LEVANT: Names = Names {
    months: LEVANT_MONTHS,
    months_short: LEVANT_MONTHS,
    days: ARABIC_DAYS,
    days_short: ARABIC_DAYS,
    am: AM_ARABIC,
    pm: PM_ARABIC,
};

pub(crate) static HIJRI: Names = Names {
    months: HIJRI_MONTHS,
    months_short: HIJRI_MONTHS,
    days: ARABIC_DAYS,
    days_short: ARABIC_DAYS,
    am: AM_ARABIC,
    pm: PM_ARABIC,
};

pub(crate) static HIJRI_EGYPT: Names = Names {
    months: HIJRI_MONTHS_EGYPT,
    months_short: HIJRI_MONTHS_EGYPT,
    days: ARABIC_DAYS,
    days_short: ARABIC_DAYS,
    am: AM_ARABIC,
    pm: PM_ARABIC,
};

pub(crate) static HIJRI_LATIN: Names = Names {
    months: HIJRI_MONTHS_LATIN,
    months_short: HIJRI_MONTHS_LATIN,
    days: [
        "AlAhad",
        "AlEthnien",
        "AthThulatha",
        "AlArbia'a",
        "AlKhamis",
        "AlJumaa",
        "AsSabt",
    ],
    days_short: [
        "AlAhad",
        "AlEthnien",
        "AthThulatha",
        "AlArbia'a",
        "AlKhamis",
        "AlJumaa",
        "AsSabt",
    ],
    am: "AM",
    pm: "PM",
};

static FRENCH: Names = Names {
    months: [
        "janvier",
        "février",
        "mars",
        "avril",
        "mai",
        "juin",
        "juillet",
        "août",
        "septembre",
        "octobre",
        "novembre",
        "décembre",
    ],
    months_short: [
        "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
        "déc.",
    ],
    days: [
        "dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi",
    ],
    days_short: ["dim.", "lun.", "mar.", "mer.", "jeu.", "ven.", "sam."],
    am: "AM",
    pm: "PM",
};

static GERMAN: Names = Names {
    months: [
        "Januar",
        "Februar",
        "März",
        "April",
        "Mai",
        "Juni",
        "Juli",
        "August",
        "September",
        "Oktober",
        "November",
        "Dezember",
    ],
    months_short: [
        "Jan", "Feb", "Mrz", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez",
    ],
    days: [
        "Sonntag",
        "Montag",
        "Dienstag",
        "Mittwoch",
        "Donnerstag",
        "Freitag",
        "Samstag",
    ],
    days_short: ["So", "Mo", "Di", "Mi", "Do", "Fr", "Sa"],
    am: "AM",
    pm: "PM",
};

static SPANISH: Names = Names {
    months: [
        "enero",
        "febrero",
        "marzo",
        "abril",
        "mayo",
        "junio",
        "julio",
        "agosto",
        "septiembre",
        "octubre",
        "noviembre",
        "diciembre",
    ],
    months_short: [
        "ene.", "feb.", "mar.", "abr.", "may.", "jun.", "jul.", "ago.", "sep.", "oct.", "nov.",
        "dic.",
    ],
    days: [
        "domingo",
        "lunes",
        "martes",
        "miércoles",
        "jueves",
        "viernes",
        "sábado",
    ],
    days_short: ["do.", "lu.", "ma.", "mi.", "ju.", "vi.", "sá."],
    am: "AM",
    pm: "PM",
};

static ITALIAN: Names = Names {
    months: [
        "gennaio",
        "febbraio",
        "marzo",
        "aprile",
        "maggio",
        "giugno",
        "luglio",
        "agosto",
        "settembre",
        "ottobre",
        "novembre",
        "dicembre",
    ],
    months_short: [
        "gen", "feb", "mar", "apr", "mag", "giu", "lug", "ago", "set", "ott", "nov", "dic",
    ],
    days: [
        "domenica",
        "lunedì",
        "martedì",
        "mercoledì",
        "giovedì",
        "venerdì",
        "sabato",
    ],
    days_short: ["dom", "lun", "mar", "mer", "gio", "ven", "sab"],
    am: "AM",
    pm: "PM",
};

static PORTUGUESE: Names = Names {
    months: [
        "janeiro",
        "fevereiro",
        "março",
        "abril",
        "maio",
        "junho",
        "julho",
        "agosto",
        "setembro",
        "outubro",
        "novembro",
        "dezembro",
    ],
    months_short: [
        "jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez",
    ],
    days: [
        "domingo",
        "segunda-feira",
        "terça-feira",
        "quarta-feira",
        "quinta-feira",
        "sexta-feira",
        "sábado",
    ],
    days_short: ["dom", "seg", "ter", "qua", "qui", "sex", "sáb"],
    am: "AM",
    pm: "PM",
};

static RUSSIAN: Names = Names {
    months: [
        "Январь",
        "Февраль",
        "Март",
        "Апрель",
        "Май",
        "Июнь",
        "Июль",
        "Август",
        "Сентябрь",
        "Октябрь",
        "Ноябрь",
        "Декабрь",
    ],
    months_short: [
        "янв", "фев", "мар", "апр", "май", "июн", "июл", "авг", "сен", "окт", "ноя", "дек",
    ],
    days: [
        "воскресенье",
        "понедельник",
        "вторник",
        "среда",
        "четверг",
        "пятница",
        "суббота",
    ],
    days_short: ["Вс", "Пн", "Вт", "Ср", "Чт", "Пт", "Сб"],
    am: "AM",
    pm: "PM",
};

static TURKISH: Names = Names {
    months: [
        "Ocak", "Şubat", "Mart", "Nisan", "Mayıs", "Haziran", "Temmuz", "Ağustos", "Eylül", "Ekim",
        "Kasım", "Aralık",
    ],
    months_short: [
        "Oca", "Şub", "Mar", "Nis", "May", "Haz", "Tem", "Ağu", "Eyl", "Eki", "Kas", "Ara",
    ],
    days: [
        "Pazar",
        "Pazartesi",
        "Salı",
        "Çarşamba",
        "Perşembe",
        "Cuma",
        "Cumartesi",
    ],
    days_short: ["Paz", "Pzt", "Sal", "Çar", "Per", "Cum", "Cmt"],
    am: "ÖÖ",
    pm: "ÖS",
};

static JAPANESE: Names = Names {
    months: [
        "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月",
    ],
    months_short: [
        "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12",
    ],
    days: [
        "日曜日",
        "月曜日",
        "火曜日",
        "水曜日",
        "木曜日",
        "金曜日",
        "土曜日",
    ],
    days_short: ["日", "月", "火", "水", "木", "金", "土"],
    am: "午前",
    pm: "午後",
};

static KOREAN: Names = Names {
    months: [
        "1월", "2월", "3월", "4월", "5월", "6월", "7월", "8월", "9월", "10월", "11월", "12월",
    ],
    months_short: [
        "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12",
    ],
    days: [
        "일요일",
        "월요일",
        "화요일",
        "수요일",
        "목요일",
        "금요일",
        "토요일",
    ],
    days_short: ["일", "월", "화", "수", "목", "금", "토"],
    am: "오전",
    pm: "오후",
};

static CHINESE_SIMPLIFIED: Names = Names {
    months: [
        "一月",
        "二月",
        "三月",
        "四月",
        "五月",
        "六月",
        "七月",
        "八月",
        "九月",
        "十月",
        "十一月",
        "十二月",
    ],
    months_short: [
        "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月",
    ],
    days: [
        "星期日",
        "星期一",
        "星期二",
        "星期三",
        "星期四",
        "星期五",
        "星期六",
    ],
    days_short: ["周日", "周一", "周二", "周三", "周四", "周五", "周六"],
    am: "上午",
    pm: "下午",
};

static CHINESE_TRADITIONAL: Names = Names {
    months: [
        "一月",
        "二月",
        "三月",
        "四月",
        "五月",
        "六月",
        "七月",
        "八月",
        "九月",
        "十月",
        "十一月",
        "十二月",
    ],
    months_short: [
        "一月",
        "二月",
        "三月",
        "四月",
        "五月",
        "六月",
        "七月",
        "八月",
        "九月",
        "十月",
        "十一月",
        "十二月",
    ],
    days: [
        "星期日",
        "星期一",
        "星期二",
        "星期三",
        "星期四",
        "星期五",
        "星期六",
    ],
    days_short: ["週日", "週一", "週二", "週三", "週四", "週五", "週六"],
    am: "上午",
    pm: "下午",
};

static SERBIAN_CYRILLIC: Names = Names {
    months: [
        "јануар",
        "фебруар",
        "март",
        "април",
        "мај",
        "јуни",
        "јули",
        "август",
        "септембар",
        "октобар",
        "новембар",
        "децембар",
    ],
    months_short: [
        "јан", "феб", "мар", "апр", "мај", "јун", "јул", "авг", "сеп", "окт", "нов", "дец",
    ],
    days: [
        "недјеља",
        "понедјељак",
        "уторак",
        "сриједа",
        "четвртак",
        "петак",
        "субота",
    ],
    days_short: ["нед", "пон", "уто", "сри", "чет", "пет", "суб"],
    am: "AM",
    pm: "PM",
};

const PRIMARY_ARABIC: u32 = 0x01;

/// Iraq, Syria, Jordan and Lebanon name the Gregorian months differently.
fn is_levant(lcid: u32) -> bool {
    matches!(lcid, 0x0801 | 0x2801 | 0x2C01 | 0x3001)
}

/// The names for a language tag. Languages not in the table get English ones.
pub(crate) fn names_for(lcid: u32) -> &'static Names {
    match lcid & 0x3ff {
        PRIMARY_ARABIC if is_levant(lcid) => &ARABIC_LEVANT,
        PRIMARY_ARABIC => &ARABIC,
        0x0c => &FRENCH,
        0x07 => &GERMAN,
        0x0a => &SPANISH,
        0x10 => &ITALIAN,
        0x16 => &PORTUGUESE,
        0x19 => &RUSSIAN,
        0x1f => &TURKISH,
        0x11 => &JAPANESE,
        0x12 => &KOREAN,
        0x04 if matches!(lcid, 0x0404 | 0x0C04 | 0x1404) => &CHINESE_TRADITIONAL,
        0x04 => &CHINESE_SIMPLIFIED,
        0x1a if matches!(lcid, 0x0C1A | 0x1C1A) => &SERBIAN_CYRILLIC,
        _ => &ENGLISH,
    }
}

/// The names of the Hijri months for a language tag, where zero means no language was named.
pub(crate) fn hijri_names_for(lcid: u32) -> &'static Names {
    match lcid & 0x3ff {
        0 => &HIJRI,
        PRIMARY_ARABIC if matches!(lcid, 0x0C01 | 0x1001 | 0x1401) => &HIJRI_EGYPT,
        PRIMARY_ARABIC => &HIJRI,
        _ => &HIJRI_LATIN,
    }
}

/// The LCID of a language tag written out, such as `ar-SA`, for the ones Excel maps itself.
pub(crate) fn lcid_from_name(name: &str) -> Option<u32> {
    let known = [
        ("en-US", 0x409),
        ("en-GB", 0x809),
        ("ar-SA", 0x401),
        ("ar-EG", 0xC01),
        ("ar-IQ", 0x801),
        ("ar-AE", 0x3801),
        ("ar-KW", 0x3401),
        ("ar-QA", 0x4001),
        ("ar-BH", 0x3C01),
        ("ar-OM", 0x2001),
        ("ar-JO", 0x2C01),
        ("ar-LB", 0x3001),
        ("ar-SY", 0x2801),
        ("fr-FR", 0x40C),
        ("de-DE", 0x407),
        ("es-ES", 0xC0A),
        ("it-IT", 0x410),
        ("pt-BR", 0x416),
        ("ru-RU", 0x419),
        ("ja-JP", 0x411),
        ("zh-CN", 0x804),
        ("zh-TW", 0x404),
        ("ko-KR", 0x412),
    ];
    known
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(name))
        .map(|(_, lcid)| *lcid)
}
