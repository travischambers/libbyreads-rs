#[cfg(feature = "ssr")]
use tokio;

use crate::error_template::{AppError, ErrorTemplate};
use leptos::*;
use leptos_meta::*;
use leptos_router::*;

use std::sync::{Arc, Mutex};

use reqwest::Client;
use scraper::{Html, Selector};
use serde_json::Value;
use urlencoding::encode;


#[derive(Debug)]
#[derive(Clone)]
#[derive(serde::Serialize)]
#[derive(serde::Deserialize)]

pub struct GoodreadsBook {
    cover: String,
    title: String,
    author: String,
    date_added: String,
}


#[server(GetGoodreadsBooks, "/goodreads-books")]
pub async fn get_goodreads_books() -> Result<Vec<GoodreadsBook>, ServerFnError> {
    let books = Arc::new(Mutex::new(Vec::new()));
    // URL of the user's to-read shelf
    // print=true here gives us a simpler webpage to parse
    // order=d sorts by descending
    // sort=date_added sorts by the order the books were added
    // TODO: get per_page to work. right now i always get 20
    // per_page=500 gives us 500 books at once. we could do more, but probably not necessary
    // TODO: make the shelf configurable via leptos multiselect dropdown
    let url = "https://goodreads.com/review/list/44369181-travis-chambers?order=d&print=true&ref=nav_mybooks&shelf=to-read&sort=date_added";

    // Parse the HTML document
    // the Html struct is not Sync, so we can't share it between threads
    // instead, we parse the document in a blocking tokio task
    let client = Client::new();
    let response = client.get(url).send().await?.text().await?;
    let last_page = tokio::task::spawn_blocking(move || {
        let html = Html::parse_document(&response);
        println!("Parsed html successfully.");
        // get the total number of pages
        let pagination_selector = Selector::parse("#reviewPagination a").unwrap();

        // Find the highest number in the pagination links
        let mut last_page = html
            .select(&pagination_selector)
            .filter_map(|element| element.text().collect::<String>().parse::<u32>().ok())
            .max()
            .unwrap_or(1); // If there are no pagination links, there is only one page
        println!("Total pages: {}", last_page);
        // TODO: remove this when i asyncify library/book searches
        if last_page > 2 {
            // only fetch the first 2 pages for now
            last_page = 2;
            println!("Only fetching the first 2 pages for now.");
        }
        last_page
    }).await?;


    // Create async tasks for each page
    let mut tasks = vec![];
    for page_number in 1..=last_page {
        let books = Arc::clone(&books); // Clone the Arc for each task
        let client = Client::new();
        let page_url = format!("{}&page={}", url, page_number);

        // Spawn a new async task to fetch and parse the page
        let task = tokio::task::spawn(async move {
            if let Ok(response) = client.get(&page_url).send().await {
                if let Ok(text) = response.text().await {
                    let document = Html::parse_document(&text);
                    // Define selectors - i just looked at the HTML directly to determine these
                    let book_rows_selector = Selector::parse("tr.bookalike.review").unwrap();
                    let cover_selector = Selector::parse("td.field.cover img").unwrap();
                    let title_selector = Selector::parse("td.field.title a").unwrap();
                    let author_selector = Selector::parse("td.field.author a").unwrap();
                    let date_added_selector = Selector::parse("td.field.date_added span").unwrap();

                    // Loop through each book row
                    for book_row in document.select(&book_rows_selector) {
                        // Get cover image
                        let cover_element = book_row.select(&cover_selector).next().unwrap();
                        let cover = cover_element.value().attr("src").unwrap().to_string();

                        // Get title
                        let title_element = book_row.select(&title_selector).next().unwrap();
                        let title = title_element.inner_html().trim().to_string();

                        // Get author
                        let author_element = book_row.select(&author_selector).next().unwrap();
                        let author = author_element.inner_html().trim().to_string();

                        // Get date added
                        let date_added_element =
                            book_row.select(&date_added_selector).next().unwrap();
                        let date_added = date_added_element.inner_html().trim().to_string();

                        // Create a book struct
                        let book = GoodreadsBook {
                            cover,
                            title,
                            author,
                            date_added,
                        };

                        // Add the book to the shared vector
                        let mut books_guard = books.lock().unwrap();
                        books_guard.push(book);
                    }
                }
            }
        });
        tasks.push(task);
    }

    // Await all tasks
    for task in tasks {
        task.await?;
    }

    // Print all the books
    let books: std::sync::MutexGuard<'_, Vec<GoodreadsBook>> = books.lock().unwrap();
    for book in books.iter() {
        println!("{:?}", book);
    }

    println!("Total number of books: {}", books.len());
    Ok(books.clone())
}

// TODO: add get_libby_availability function and then add libby availability to each row in the table


#[component]
pub fn App() -> impl IntoView {
    // Provides context that manages stylesheets, titles, meta tags, etc.
    provide_meta_context();

    view! {


        // injects a stylesheet into the document <head>
        // id=leptos means cargo-leptos will hot-reload this stylesheet
        <Stylesheet id="leptos" href="/pkg/libbyreads-rs.css"/>

        // sets the document title
        <Title text="Welcome to Leptos"/>

        // content for this welcome page
        <Router fallback=|| {
            let mut outside_errors = Errors::default();
            outside_errors.insert_with_default_key(AppError::NotFound);
            view! {
                <ErrorTemplate outside_errors/>
            }
            .into_view()
        }>
            <main>
                <Routes>
                    <Route path="" view=HomePage/>
                </Routes>
            </main>
        </Router>
    }
}

/// Renders the home page of your application.
#[component]
fn HomePage() -> impl IntoView { 
    let (books, set_books) = create_signal(Vec::new());
    let fetch_books = move |_| {
        spawn_local(async move {
            match get_goodreads_books().await {
                Ok(fetched_books) => set_books.set(fetched_books),
                Err(e) => {println!("{}", e.to_string())}, 
            }
        });
    };

    view! {
        // click button to call get_goodreads_books to fetch the books
        <button on:click=fetch_books>"Fetch Goodreads Books"</button>
        // display books in a table
        <table>
            <thead>
                <tr>
                    <th>"Cover"</th>
                    <th>"Title"</th>
                    <th>"Author"</th>
                    <th>"Libby Availability"</th>
                </tr>
            </thead>
            <tbody>
                {move || books.get().iter().map(|book| view! {
                    <tr>
                        <td><img src={book.cover.clone()} alt="cover" /></td>
                        <td>{book.title.clone()}</td>
                        <td>{book.author.clone()}</td>
                    </tr>
                }).collect::<Vec<_>>()}
            </tbody>
        </table>
    }
}
