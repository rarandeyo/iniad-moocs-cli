pub mod assignments;
pub mod moocs;
pub mod slides;

pub use assignments::{
    get_answers, get_assessment, get_assignment_detail, get_file, get_problem_html, get_status,
    post_file, put_answers,
};
pub use moocs::{
    get_course_detail, get_course_list, get_lesson_page, get_lesson_with_assignments,
    list_course_assignments, resolve_latest_year,
};
