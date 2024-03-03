#[cfg(test)]
pub const INHIBIT_TEST: &str = "UEsDBAoAAAAAAHO7VljZLlTQMAIAADACAAAKAAAAbW9kZWwuanNvbnsKICAibW9kZWxUeXBlIjogInBldHJpTmV0IiwKICAidmVyc2lvbiI6ICJ2MCIsCiAgInBsYWNlcyI6IHsKICAgICJwbGFjZTAiOiB7ICJvZmZzZXQiOiAwLCAiaW5pdGlhbCI6IDEsICJjYXBhY2l0eSI6IDMsICJ4IjogNzY1LCAieSI6IDI5NCB9CiAgfSwKICAidHJhbnNpdGlvbnMiOiB7CiAgICAidHhuMCI6IHsgIngiOiA2NTksICJ5IjogMjI2IH0sCiAgICAidHhuMSI6IHsgIngiOiA4NjQsICJ5IjogMjIxIH0sCiAgICAidHhuMiI6IHsgIngiOiA2NTIsICJ5IjogMzU4IH0sCiAgICAidHhuMyI6IHsgIngiOiA4NjYsICJ5IjogMzY0IH0KICB9LAogICJhcmNzIjogWwogICAgeyAic291cmNlIjogInR4bjAiLCAidGFyZ2V0IjogInBsYWNlMCIgfSwKICAgIHsgInNvdXJjZSI6ICJwbGFjZTAiLCAidGFyZ2V0IjogInR4bjEiIH0sCiAgICB7ICJzb3VyY2UiOiAidHhuMiIsICJ0YXJnZXQiOiAicGxhY2UwIiwgIndlaWdodCI6IDMsICJpbmhpYml0IjogdHJ1ZSB9LAogICAgeyAic291cmNlIjogInBsYWNlMCIsICJ0YXJnZXQiOiAidHhuMyIsICJpbmhpYml0IjogdHJ1ZSB9CiAgXQp9UEsBAhQACgAAAAAAc7tWWNkuVNAwAgAAMAIAAAoAAAAAAAAAAAAAAAAAAAAAAG1vZGVsLmpzb25QSwUGAAAAAAEAAQA4AAAAWAIAAAAA";

#[cfg(test)]
pub const RANDO_TEST: &str = "UEsDBAoAAAAAAGwXV1h1ub+YPAEAADwBAAAKAAAAbW9kZWwuanNvbnsKICAibW9kZWxUeXBlIjogInBldHJpTmV0IiwKICAidmVyc2lvbiI6ICJ2MCIsCiAgInBsYWNlcyI6IHsKICAgICJwbGFjZTAiOiB7ICJvZmZzZXQiOiAwLCAieCI6IDUzMCwgInkiOiAzMTUgfSwKICAgICJwbGFjZTEiOiB7ICJvZmZzZXQiOiAxLCAieCI6IDUyNCwgInkiOiA0NDYgfQogIH0sCiAgInRyYW5zaXRpb25zIjogewogICAgInR4bjAiOiB7ICJ4IjogMjQ0LCAieSI6IDIwMCB9LAogICAgInR4bjEiOiB7ICJ4IjogMzUyLCAieSI6IDIzNyB9LAogICAgInR4bjIiOiB7ICJ4IjogMjY2LCAieSI6IDQ3OSB9CiAgfSwKICAiYXJjcyI6IFsKICBdCn1QSwECFAAKAAAAAABsF1dYdbm/mDwBAAA8AQAACgAAAAAAAAAAAAAAAAAAAAAAbW9kZWwuanNvblBLBQYAAAAAAQABADgAAABkAQAAAAA=";

#[cfg(test)]
pub const DINING_PHILOSOPHERS: &'static str = r#"
{
    "modelType": "petriNet",
    "version": "v0",
    "places": {
        "right2": { "offset": 0, "x": 810, "y": 149 },
        "left2": { "offset": 1, "x": 942, "y": 153 },
        "right3": { "offset": 2, "x": 1182, "y": 218 },
        "left3": { "offset": 3, "x": 1260, "y": 339 },
        "right4": { "offset": 4, "x": 1169, "y": 744 },
        "left4": { "offset": 5, "x": 1082, "y": 843 },
        "right5": { "offset": 6, "x": 630, "y": 856 },
        "left5": { "offset": 7, "x": 531, "y": 728 },
        "right1": { "offset": 8, "x": 441, "y": 359 },
        "left1": { "offset": 9, "x": 501, "y": 244 },
        "chopstick1": { "offset": 10, "initial": 1, "x": 811, "y": 426 },
        "chopstick2": { "offset": 11, "initial": 1, "x": 931, "y": 434 },
        "chopstick3": { "offset": 12, "initial": 1, "x": 969, "y": 545 },
        "chopstick4": { "offset": 13, "initial": 1, "x": 863, "y": 614 },
        "chopstick5": { "offset": 14, "initial": 1, "x": 774, "y": 536 }
    },
    "transitions": {
        "eat1": { "x": 610, "y": 370 },
        "think1": { "x": 372, "y": 247 },
        "eat2": { "x": 874, "y": 281 },
        "think2": { "x": 876, "y": 42 },
        "eat3": { "x": 1115, "y": 348 },
        "think3": { "x": 1309, "y": 215 },
        "eat4": { "x": 1034, "y": 691 },
        "think4": { "x": 1227, "y": 896 },
        "eat5": { "x": 673, "y": 688 },
        "think5": { "x": 483, "y": 887 }
    },
    "arcs": [
        { "source": "chopstick1", "target": "eat1" },
        { "source": "chopstick5", "target": "eat1" },
        { "source": "eat1", "target": "left1" },
        { "source": "eat1", "target": "right1" },
        { "source": "eat2", "target": "right2" },
        { "source": "eat2", "target": "left2" },
        { "source": "chopstick1", "target": "eat2" },
        { "source": "chopstick2", "target": "eat2" },
        { "source": "chopstick2", "target": "eat3" },
        { "source": "chopstick3", "target": "eat3" },
        { "source": "eat3", "target": "right3" },
        { "source": "eat3", "target": "left3" },
        { "source": "chopstick3", "target": "eat4" },
        { "source": "chopstick4", "target": "eat4" },
        { "source": "eat4", "target": "left4" },
        { "source": "eat4", "target": "right4" },
        { "source": "think4", "target": "chopstick4" },
        { "source": "think4", "target": "chopstick3" },
        { "source": "right4", "target": "think4" },
        { "source": "left4", "target": "think4" },
        { "source": "chopstick5", "target": "eat5" },
        { "source": "chopstick4", "target": "eat5" },
        { "source": "eat5", "target": "left5" },
        { "source": "eat5", "target": "right5" },
        { "source": "think5", "target": "chopstick5" },
        { "source": "think5", "target": "chopstick4" },
        { "source": "left5", "target": "think5" },
        { "source": "right5", "target": "think5" },
        { "source": "left1", "target": "think1" },
        { "source": "right1", "target": "think1" },
        { "source": "think2", "target": "chopstick1" },
        { "source": "think2", "target": "chopstick2" },
        { "source": "think1", "target": "chopstick1" },
        { "source": "think1", "target": "chopstick5" },
        { "source": "right3", "target": "think3" },
        { "source": "left3", "target": "think3" },
        { "source": "think3", "target": "chopstick2" },
        { "source": "think3", "target": "chopstick3" },
        { "source": "right2", "target": "think2" },
        { "source": "left2", "target": "think2" }
    ]
}"#;