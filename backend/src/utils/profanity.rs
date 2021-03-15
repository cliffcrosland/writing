use aho_corasick::AhoCorasick;
use lazy_static::lazy_static;

pub fn contains_profanity(s: &str) -> bool {
    let lower_case = s.to_ascii_lowercase();
    PROFANITY_MATCHER.is_match(lower_case)
}

lazy_static! {
    static ref PROFANITY_MATCHER: AhoCorasick = AhoCorasick::new_auto_configured(&PROFANE_WORDS);
    static ref PROFANE_WORDS: Vec<&'static str> = vec![
        // English:
        "shit",
        "sh1t",
        "5hit",
        "5h1t",
        "fuc",
        "fuk",
        "dam",
        "d4m",
        "ass",
        "as5",
        "a5s",
        "a55",
        "4ss",
        "4s5",
        "nig",
        "n1g",
        "coc",
        "c0c",
        "cok",
        "c0k",
        "suc",
        "suk",
        "dic",
        "d1c",
        "dik",
        "d1k",
        "vagin",
        "vag1n",
        "v4gin",
        "v4g1n",
        "boob",
        "bo0b",
        "b0ob",
        "b00b",
        "tit",
        "t1t",
        "anal",
        "an4l",
        "4nal",
        "4n4l",
        "arse",
        "ars3",
        "4rse",
        "4rs3",
        "bitch",
        "bi7ch",
        "b1tch",
        "b17ch",
        "pedo",
        "ped0",
        "p3do",
        "p3d0",
        "ball",
        "b4ll",
        "clit",
        "cl1t",
        "cun",
        "jerk",
        "j3rk",
        "porn",
        "p0rn",
        "pron",
        "pr0n",
        "puss",
        "pus5",
        "pu5s",
        "pu55",
        "sex",
        "s3x",
        "5ex",
        "53x",
        "bastar",
        "bast4r",
        "b4star",
        "b4st4r",
        "bum",
        "butt",
        // French:
        "merd",
        "m3rd",
        "pute",
        "put3",
        "putain",
        "puta1n",
        "put4in",
        "put41n",
        "bais",
        "ba1s",
        "b4is",
        "b41s",
        "nique",
        "niqu3",
        "n1que",
        "n1qu3",
        // Spanish:
        "cabron",
        "cabr0n",
        "c4bron",
        "c4br0n",
        "joder",
        "jod3r",
        "j0der",
        "j0d3r",
        "mierda",
        "mierd4",
        "mi3rda",
        "mi3rd4",
        "m1erda",
        "m1erd4",
        "m13rda",
        "m13rd4",
        "cojone",
        "cojon3",
        "coj0ne",
        "coj0n3",
        "c0jone",
        "c0jon3",
        "c0j0ne",
        "c0j0n3",
        "cago",
        "cag0",
        "c4go",
        "c4g0",
    ];
}

#[cfg(test)]
mod tests{
    use super::*;

    #[test]
    fn test_profanity_check() {
        // "...fUK..."
        assert!(contains_profanity("u_123odjcmdfUKuw9DITN3Ld"));
        // "...5H1t..."
        assert!(contains_profanity("o_5H1tdkgj9838NUEP3kl34s"));
        // no profanity
        assert!(!contains_profanity("o_QCar3LwOwBPIeKonywpCpB"));
    }
}
