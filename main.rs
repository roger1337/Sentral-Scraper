use std::collections::HashMap;
use std::error::Error;
use std::io::{Read, stdin};
use std::ops::{Add, Range, RangeInclusive};
use chrono::{Datelike, Duration, Utc};
use rand::distributions::Alphanumeric;
use rand::Rng;
use reqwest::{Client};
use scraper::{ElementRef, Html};
use scraper::node::Element;

struct Period {
    subject: String,
    classroom: String,
    class_id: String,
    teacher: String,
    period_number: String,
}

impl Period {
    fn new(subject: String, classroom: String, class_id: String, teacher: String, period_number: String) -> Self {
        Period { subject, classroom, class_id, teacher, period_number }
    }

    fn none(period_number: String) -> Self {
        Period {
            subject: "None".to_owned(),
            classroom: Default::default(),
            class_id: Default::default(),
            teacher: Default::default(),
            period_number,
        }
    }
}

struct TimetableDayData {
    date: String,
    weekday: String,
    periods: Vec<Period>
}

impl TimetableDayData {
    fn new(date: String, weekday: String, periods: Vec<Period>) -> Self {
        TimetableDayData { date, weekday, periods }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let login = get_login_body("roger", "");
    let portal_sid = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(100)
        .map(char::from)
        .collect::<String>();


    println!("Authenticating SID...");

    let client = Client::new();
    let sid_res = client
        .post("https://SCHOOL.sentral.com.au/portal2/user")
        .header("Cookie", format!("PortalSID={}", portal_sid))
        .json(&login)
        .send()
        .await?;

    if sid_res.status() != 200 {
        println!("SID Request Error (Invalid Credentials)");
        return Ok(());
    }


    let normal_link_res = client
        .get("https://SCHOOL.sentral.com.au/portal/timetable/mytimetable/")
        .header("Cookie", format!("PortalSID={}", portal_sid))
        .send()
        .await?;

    let text = normal_link_res.text().await?;
    let number = scrape_daily_link(text)?.split("/").nth(4).unwrap().parse::<i32>().unwrap();

    let timetable_res = client
        .get(format!("https://SCHOOL.sentral.com.au/portal/timetable/mytimetable/{}/daily", number))
        .header("Cookie", format!("PortalSID={}", portal_sid))
        .send()
        .await?;


    let text = timetable_res.text().await?;
    let next = scrape_timetable(text.clone(), vec![1,2,3])?;

    let prev_res = scrape_timetable(text, vec![0, -1, -2, -3]);
    let prev = if prev_res.is_ok() {
        prev_res.unwrap()
    } else {
        println!("previous not present in current daily page, retrying...");

        let timetable_res = client
            .get(format!("https://SCHOOL.sentral.com.au/portal/timetable/mytimetable/{}/daily", number - 1))
            .header("Cookie", format!("PortalSID={}", portal_sid))
            .send()
            .await?;

        let text = timetable_res.text().await?;
        scrape_timetable(text, vec![0, -1, -2, -3])?
    };


    println!();
    println!("Timetable for {} ({})", next.date, next.weekday);
    for period in &next.periods {
        if period.subject != "None" {
            println!("{:<2} - {:<25} {:<10} in {:<4} with {}", period.period_number, period.subject,  period.class_id, period.classroom, period.teacher)
        }
    }

    let mut new_periods = Vec::new();
    let mut old_periods = Vec::new();

    for period in &next.periods {
        if !prev.periods.iter().any(|x| period.subject == x.subject || period.subject == "None") {
            new_periods.push(period);
        }
    }

    for period in &prev.periods {
        if !next.periods.iter().any(|x| period.subject == x.subject || period.subject == "None") {
            old_periods.push(period);
        }
    }
    println!();
    println!("In comparison to previous timetable {} ({})", prev.date, prev.weekday);

    for period in new_periods {
        println!(" + {}", period.subject);
    }

    for period in old_periods {
        println!(" - {}", period.subject);
    }



    println!();

    println!("Press Enter to exit...");

    let mut input = [];
    stdin().read(&mut input)
        .ok()
        .expect("Failed to read line");

    Ok(())
}

fn scrape_daily_link(html: String) -> Result<String, Box<dyn Error>> {
    let document = Html::parse_document(html.as_str());
    let selector = scraper::Selector::parse("i.icon-certificate").unwrap();


    let link = ElementRef::wrap(document.select(&selector).nth(0).unwrap().parent().unwrap()).unwrap();
    Ok(link.value().attr("href").unwrap().to_owned())
}

fn scrape_timetable(html: String, offset_vec: Vec<i64>) -> Result<TimetableDayData, Box<dyn Error>> {
    let mut res = Vec::new();
    let document = Html::parse_document(html.as_str());
    let period_row_selector = scraper::Selector::parse("tr").unwrap();
    let is_period_row_selector = scraper::Selector::parse("th.timetable-period").unwrap();
    let date_selector = scraper::Selector::parse("th.timetable-date").unwrap();

    let mut rows = document.select(&period_row_selector).filter(|x| x.select(&is_period_row_selector).count() > 0).collect::<Vec<ElementRef>>();
    let dates = document.select(&date_selector).map(|x| x.inner_html()).filter(|x| x.contains("/")).collect::<Vec<String>>();

    let mut day_number = None;
    let mut timetable_date = None;
    let mut timetable_date_day = None;

    'date_loop:
    for offset in offset_vec {

        let current_date = Utc::today().add(Duration::days(offset as i64));
        let current_date_str = current_date.format("%d/%m/%Y").to_string();

        for (i, date) in dates.iter().enumerate() {
            if date == &current_date_str {
                day_number = Some(i);
                timetable_date = Some(date);
                timetable_date_day = Some(current_date.weekday().to_string());
                break 'date_loop;
            }
        }
    }

    let mut day_number = day_number.ok_or("could not find day number through the date.")?;

    if day_number > 4 {
        rows.drain(0..12);
        day_number -= 5;
    }


    let period_number_selector = scraper::Selector::parse("th.timetable-period").unwrap();
    let class_name_selector = scraper::Selector::parse("div.timetable-class").unwrap();

    'row_loop:
    for element in rows {
        let period_number = element.select(&period_number_selector).nth(0).ok_or("Cannot find period number").map(|x| x.inner_html().trim().to_owned()).unwrap();

        for (i, period) in element.children()
            .map(|child| ElementRef::wrap(child))
            .filter(|elm| elm.is_some() && !elm.unwrap().value().classes().any(|c| c == "timetable-period")).enumerate() {

            if let Some(period) = period {
                if i == day_number {

                    if period.value().classes().any(|c| c == "inactive") {
                        res.push(Period::none(period_number.clone()));
                        continue 'row_loop;
                    }

                    let mut text_iter = period.select(&class_name_selector).nth(0).ok_or("Cannot find period data").map(|x| x.text()).unwrap();

                    let class_name = text_iter.nth(2).unwrap().trim();
                    let class_id = text_iter.next().unwrap().trim();
                    let class_room = text_iter.nth(2).unwrap().trim();
                    let teacher = text_iter.nth(1).unwrap().trim();

                    res.push(Period::new(class_name.to_owned(), class_room.to_owned(), class_id.to_owned(), teacher.to_owned(), period_number.clone()));
                }
            }
        }
    }

    Ok(TimetableDayData::new(timetable_date.unwrap().to_string(), timetable_date_day.unwrap(), res))
}




fn get_login_body<'a>(username: &'a str, password: &'a str) -> HashMap<&'a str, &'a str> {
    let mut map = HashMap::new();
    map.insert("action", "login");
    map.insert("password", password);
    map.insert("username", username);
    map.insert("remember_username", "false");
    map
}
