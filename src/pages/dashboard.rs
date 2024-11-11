use crate::components::dashboard::*;
use leptos::prelude::*;

#[component]
pub fn DashboardPage() -> impl IntoView {
    view! {
        <section class="container">
            <h1>"Dashboard"</h1>
            <div>
                <div>
                    <Interface/>
                    <Resume/>
                </div>
                <TableGroupsUser/>
            </div>
            <Traffics/>
        </section>
    }
}
